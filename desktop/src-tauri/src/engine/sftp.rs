//! SFTP 下载（SSH）：用户名密码或本机的 ~/.ssh 无口令私钥登录，首次连接记住服务器指纹（TOFU），
//! 之后指纹变化会拒绝连接。支持断点续传、限速、暂停 / 取消。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handler};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use url::Url;

use super::http::HttpOptions;
use super::{ensure_space, wait_ctrl, DlError, ResumeMeta};

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 已信任的服务器指纹：`host:port` → `SHA256:...`
#[derive(Clone)]
pub struct KnownHosts {
    path: PathBuf,
}

impl KnownHosts {
    pub fn new(path: PathBuf) -> Self {
        KnownHosts { path }
    }

    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 首次见到的服务器记下指纹并信任；已记录的必须一致。
    fn check(&self, host: &str, fingerprint: &str) -> Result<(), String> {
        let mut all = self.load();
        match all.get(host) {
            Some(known) if known == fingerprint => Ok(()),
            Some(known) => Err(format!(
                "服务器 {host} 的主机密钥与上次连接时不同（原来是 {known}，现在是 {fingerprint}）。可能是服务器重装了，也可能有人在冒充它；确认无误后，删除“数据目录 → sftp_known_hosts.json”里这一行再重试。"
            )),
            None => {
                all.insert(host.to_string(), fingerprint.to_string());
                if let Some(dir) = self.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&self.path, serde_json::to_string_pretty(&all).unwrap_or_default());
                Ok(())
            }
        }
    }
}

struct Client {
    host: String,
    known: KnownHosts,
    rejected: Arc<Mutex<Option<String>>>,
}

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fp = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(_) => {
                *self.rejected.lock().unwrap_or_else(|e| e.into_inner()) = Some("服务器使用了证书形式的主机密钥，暂不支持。".into());
                return Ok(false);
            }
        };
        match self.known.check(&self.host, &fp) {
            Ok(()) => Ok(true),
            Err(msg) => {
                *self.rejected.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
                Ok(false)
            }
        }
    }
}

fn net<E: std::fmt::Display>(e: E) -> DlError {
    DlError::Net(e.to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// 密码登录；没有密码时依次尝试本机无口令的默认私钥。
async fn authenticate(handle: &mut client::Handle<Client>, user: &str, password: &str) -> Result<(), DlError> {
    if !password.is_empty() {
        let r = handle.authenticate_password(user, password).await.map_err(net)?;
        return if r.success() { Ok(()) } else { Err(DlError::Other("SFTP 登录失败：用户名或密码不对。".into())) };
    }
    let dir = home_dir().map(|h| h.join(".ssh"));
    let mut tried = false;
    for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        let Some(path) = dir.as_ref().map(|d| d.join(name)).filter(|p| p.is_file()) else { continue };
        let Ok(key) = russh::keys::load_secret_key(&path, None) else { continue };
        tried = true;
        let hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
        let r = handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(net)?;
        if r.success() {
            return Ok(());
        }
    }
    Err(DlError::Other(if tried {
        "SFTP 登录失败：服务器不接受本机的 SSH 密钥。可以在链接里写上密码：sftp://用户名:密码@服务器/路径".into()
    } else {
        "SFTP 需要登录：请在链接里写上用户名和密码（sftp://用户名:密码@服务器/路径），或在本机 ~/.ssh 放一个没有口令的私钥。".into()
    }))
}

/// 把远端文件复制到本地部分文件；已有部分内容时从断点继续。远端用任何可读可定位的对象表示，便于测试。
pub async fn copy_remote<R: AsyncRead + AsyncSeek + Unpin>(
    remote: &mut R,
    size: Option<u64>,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    on_progress: &mut (impl FnMut(u64, Option<u64>, &ResumeMeta) + Send),
) -> Result<u64, DlError> {
    meta.segments.clear();
    meta.total = size;
    meta.resumable = Some(true);
    let mut existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if let Some(t) = size {
        if existing == t && t > 0 {
            on_progress(t, Some(t), meta);
            return Ok(t);
        }
        if existing > t {
            existing = 0;
        }
        if let Some(dir) = part.parent() {
            ensure_space(dir, t - existing, opts.disk_reserve)?;
        }
    }
    remote.seek(std::io::SeekFrom::Start(existing)).await.map_err(net)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(existing > 0)
        .truncate(existing == 0)
        .open(part)
        .await
        .map_err(|e| DlError::Io(format!("无法写入文件 {}：{e}", part.display())))?;
    let mut received = existing;
    on_progress(received, size, meta);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = tokio::select! {
            n = tokio::time::timeout(IDLE_TIMEOUT, remote.read(&mut buf)) => {
                n.map_err(|_| DlError::Net("服务器长时间没有数据".into()))?.map_err(net)?
            }
            c = wait_ctrl(ctrl) => {
                let _ = file.flush().await;
                return Err(DlError::from_ctrl(c));
            }
        };
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
        received += n as u64;
        on_progress(received, size, meta);
        opts.throttle(n).await;
    }
    file.flush().await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
    if let Some(t) = size {
        if received < t {
            return Err(DlError::Net(format!("连接提前结束（{received}/{t} 字节）")));
        }
    }
    meta.total = Some(received);
    Ok(received)
}

#[allow(clippy::too_many_arguments)]
pub async fn download(
    url: &Url,
    credentials: Option<&str>,
    known_hosts: &KnownHosts,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    mut on_progress: impl FnMut(u64, Option<u64>, &ResumeMeta) + Send,
) -> Result<u64, DlError> {
    let host = url.host_str().ok_or_else(|| DlError::Other("链接里没有服务器地址".into()))?.to_string();
    let port = url.port().unwrap_or(22);
    let path = crate::grab::percent_decode(url.path());
    let (user, password) = match credentials {
        Some(c) => c.split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((c.to_string(), String::new())),
        None => (String::new(), String::new()),
    };
    if user.is_empty() {
        return Err(DlError::Other("SFTP 需要用户名：请使用 sftp://用户名:密码@服务器/路径。".into()));
    }

    let mut outer = ctrl.clone();
    let work = async {
        let rejected = Arc::new(Mutex::new(None));
        let handler = Client { host: format!("{host}:{port}"), known: known_hosts.clone(), rejected: rejected.clone() };
        let config = client::Config { inactivity_timeout: Some(Duration::from_secs(120)), ..Default::default() };
        let connect = tokio::time::timeout(Duration::from_secs(20), client::connect(Arc::new(config), (host.as_str(), port), handler))
            .await
            .map_err(|_| DlError::Net(format!("连接 {host}:{port} 超时")))?;
        let mut handle = match connect {
            Ok(h) => h,
            Err(e) => {
                if let Some(msg) = rejected.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    return Err(DlError::Other(msg));
                }
                return Err(DlError::Net(format!("无法连接 {host}:{port}：{e}")));
            }
        };
        authenticate(&mut handle, &user, &password).await?;
        let channel = handle.channel_open_session().await.map_err(net)?;
        channel.request_subsystem(true, "sftp").await.map_err(|e| DlError::Other(format!("服务器没有开启 SFTP：{e}")))?;
        let sftp = SftpSession::new(channel.into_stream()).await.map_err(|e| DlError::Other(format!("SFTP 初始化失败：{e}")))?;
        let size = sftp.metadata(path.clone()).await.map_err(|e| DlError::Other(format!("文件不存在或没有读取权限：{path}（{e}）")))?.size;
        let mut remote = sftp.open(path.clone()).await.map_err(|e| DlError::Other(format!("无法打开 {path}：{e}")))?;
        let r = copy_remote(&mut remote, size, part, meta, ctrl, opts, &mut on_progress).await;
        let _ = sftp.close().await;
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        r
    };
    // 连接、登录阶段也要能暂停 / 取消：外层用一份独立的接收端监听
    tokio::select! {
        r = work => r,
        c = wait_ctrl(&mut outer) => Err(DlError::from_ctrl(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cc-sftp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn content(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 253) as u8).collect()
    }

    async fn copy(data: &[u8], size: Option<u64>, part: &Path, meta: &mut ResumeMeta) -> Result<u64, DlError> {
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let mut r = Cursor::new(data.to_vec());
        copy_remote(&mut r, size, part, meta, &mut rx, &HttpOptions::default(), &mut |_, _, _| {}).await
    }

    #[tokio::test]
    async fn copies_and_resumes_from_the_partial_file() {
        let data = content(700_000);
        let part = tmp("resume");
        std::fs::write(&part, &data[..123_456]).unwrap();
        let mut meta = ResumeMeta::default();
        let n = copy(&data, Some(data.len() as u64), &part, &mut meta).await.unwrap();
        assert_eq!(n, 700_000);
        assert_eq!(std::fs::read(&part).unwrap(), data);
        assert_eq!(meta.total, Some(700_000));
    }

    #[tokio::test]
    async fn complete_part_is_kept_and_oversized_part_restarts() {
        let data = content(1000);
        let part = tmp("done");
        std::fs::write(&part, &data).unwrap();
        assert_eq!(copy(&data, Some(1000), &part, &mut ResumeMeta::default()).await.unwrap(), 1000);
        std::fs::write(&part, content(5000)).unwrap();
        copy(&data, Some(1000), &part, &mut ResumeMeta::default()).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), data);
    }

    #[tokio::test]
    async fn short_read_is_a_network_error() {
        let data = content(1000);
        let part = tmp("short");
        let err = copy(&data[..400], Some(1000), &part, &mut ResumeMeta::default()).await.unwrap_err();
        assert!(matches!(err, DlError::Net(_)), "{err}");
        // 继续时从已有的 400 字节处接着取
        assert_eq!(copy(&data, Some(1000), &part, &mut ResumeMeta::default()).await.unwrap(), 1000);
        assert_eq!(std::fs::read(&part).unwrap(), data);
    }

    #[tokio::test]
    async fn unknown_size_downloads_everything() {
        let data = content(3000);
        let part = tmp("nosize");
        let mut meta = ResumeMeta::default();
        assert_eq!(copy(&data, None, &part, &mut meta).await.unwrap(), 3000);
        assert_eq!(meta.total, Some(3000));
    }

    #[test]
    fn known_hosts_trust_on_first_use() {
        let path = tmp("known.json");
        let k = KnownHosts::new(path.clone());
        assert!(k.check("a.com:22", "SHA256:aaa").is_ok());
        assert!(k.check("a.com:22", "SHA256:aaa").is_ok());
        let err = k.check("a.com:22", "SHA256:bbb").unwrap_err();
        assert!(err.contains("SHA256:aaa") && err.contains("SHA256:bbb"));
        // 其他服务器互不影响，且记录落盘
        assert!(k.check("b.com:22", "SHA256:ccc").is_ok());
        let again = KnownHosts::new(path);
        assert!(again.check("a.com:22", "SHA256:bbb").is_err());
    }

    #[tokio::test]
    async fn missing_username_is_explained() {
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let k = KnownHosts::new(tmp("k2.json"));
        let err = download(
            &Url::parse("sftp://127.0.0.1/x").unwrap(),
            None,
            &k,
            &tmp("nouser"),
            &mut ResumeMeta::default(),
            &mut rx,
            &HttpOptions::default(),
            |_, _, _| {},
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("用户名"));
    }
}
