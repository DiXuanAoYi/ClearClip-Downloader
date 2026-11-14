//! 加密密钥管理：优先存系统钥匙串（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service），
//! 不可用时退回到应用数据目录下的密钥文件，并在界面上提示。

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};

const SERVICE: &str = "ClearClip";
const ACCOUNT: &str = "cookie-key";

pub struct KeyInfo {
    pub key: [u8; 32],
    pub in_keyring: bool,
    /// 密钥是否真的保存下来了（钥匙串或密钥文件）。false 表示这把密钥只在内存里，
    /// 用它加密的数据下次启动就解不开，所以不能拿它写保险箱。
    pub persisted: bool,
}

pub fn load_or_create_key(data_dir: &Path) -> KeyInfo {
    match keyring_key() {
        Ok(key) => KeyInfo { key, in_keyring: true, persisted: true },
        Err(e) => {
            log::warn!("system keyring unavailable, falling back to key file: {e}");
            match file_key(data_dir) {
                Ok(key) => KeyInfo { key, in_keyring: false, persisted: true },
                Err(e) => {
                    log::error!("key file unusable, secrets will not be saved this session: {e}");
                    KeyInfo { key: new_key(), in_keyring: false, persisted: false }
                }
            }
        }
    }
}

fn keyring_key() -> Result<[u8; 32], String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(hex_key) => decode_key(&hex_key).ok_or_else(|| "stored key is malformed".to_string()),
        Err(keyring::Error::NoEntry) => {
            let key = new_key();
            entry.set_password(&hex::encode(key)).map_err(|e| e.to_string())?;
            // 写入后读回确认，部分 Linux 环境写入“成功”但实际不可读
            match entry.get_password() {
                Ok(v) if decode_key(&v) == Some(key) => Ok(key),
                Ok(_) => Err("keyring read-back mismatch".into()),
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// 读取（或第一次创建）密钥文件。已有文件但内容损坏时不覆盖，返回错误，避免丢掉还能手工恢复的旧密钥；
/// 新密钥先写临时文件再改名，写不进去就返回错误，不会退回一把没存下来的内存密钥。
fn file_key(data_dir: &Path) -> Result<[u8; 32], String> {
    let path = data_dir.join("cookie.key");
    match std::fs::read_to_string(&path) {
        Ok(s) => return decode_key(s.trim()).ok_or_else(|| format!("{} 内容损坏", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("读取 {} 失败：{e}", path.display())),
    }
    let key = new_key();
    write_key_file(&path, &key).map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
    Ok(key)
}

fn write_key_file(path: &Path, key: &[u8; 32]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("key.tmp");
    let r = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        restrict_permissions(&tmp);
        f.write_all(hex::encode(key).as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

pub fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn new_key() -> [u8; 32] {
    Aes256Gcm::generate_key(OsRng).into()
}

fn decode_key(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}

/// AES-256-GCM 加密，输出 = 12 字节 nonce + 密文。
pub fn encrypt(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(&nonce, plain).expect("aes-gcm encryption cannot fail for in-memory buffers"));
    out
}

pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip_and_tamper_detection() {
        let key = new_key();
        let enc = encrypt(&key, b"SESSDATA=abc");
        assert_ne!(&enc[12..], b"SESSDATA=abc");
        assert_eq!(decrypt(&key, &enc).unwrap(), b"SESSDATA=abc");
        let mut bad = enc.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(decrypt(&key, &bad).is_none());
        assert!(decrypt(&new_key(), &enc).is_none());
    }

    #[test]
    fn file_key_is_stable() {
        let dir = std::env::temp_dir().join(format!("clearclip-key-{}", std::process::id()));
        let a = file_key(&dir).unwrap();
        let b = file_key(&dir).unwrap();
        assert_eq!(a, b);
        assert!(!dir.join("cookie.key.tmp").exists(), "临时文件不残留");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_key_never_falls_back_to_a_memory_key() {
        let dir = std::env::temp_dir().join(format!("clearclip-key-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 数据目录的位置被一个普通文件占住：写不进去，必须报错而不是返回内存密钥
        std::fs::write(&dir, "x").unwrap();
        assert!(file_key(&dir).is_err());
        std::fs::remove_file(&dir).unwrap();
        // 已有的密钥文件损坏：不覆盖，报错
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cookie.key"), "not-hex").unwrap();
        assert!(file_key(&dir).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("cookie.key")).unwrap(), "not-hex");
        let _ = std::fs::remove_dir_all(dir);
    }
}
