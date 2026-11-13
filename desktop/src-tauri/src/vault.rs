//! 密钥保险箱：API 密钥、WebDAV 密码、通知令牌等敏感信息加密保存在 `vault.bin`，
//! 不写进 settings.json，也不进入备份。密钥与 Cookie 存储共用系统钥匙串里的那一把。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use crate::error::{AppError, AppResult};
use crate::secret;

pub struct Vault {
    path: Option<PathBuf>,
    key: [u8; 32],
    data: RwLock<BTreeMap<String, String>>,
    /// 打开时发现旧文件解不开：旧文件已改名保留，第一次保存会返回这条提示（之后正常使用新文件）。
    notice: Mutex<Option<String>>,
    /// 保险箱不能写入的原因（读不了旧文件、密钥没能保存）。有值时所有写入都返回错误，避免覆盖还能恢复的数据。
    locked: Option<String>,
}

/// 名称只能用小写字母、数字和 `. _ -`，防止被拿来读写别的东西。
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

impl Vault {
    pub fn open(path: PathBuf, key: [u8; 32]) -> Vault {
        Self::open_with(path, key, None)
    }

    /// `locked` 有值表示密钥没能保存下来：可以读，但任何写入都返回这条错误。
    pub fn open_with(path: PathBuf, key: [u8; 32], locked: Option<String>) -> Vault {
        let mut data = BTreeMap::new();
        let mut notice = None;
        let mut locked = locked;
        match std::fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => {}
            Ok(bytes) => match secret::decrypt(&key, &bytes).and_then(|plain| serde_json::from_slice::<BTreeMap<String, String>>(&plain).ok()) {
                Some(d) => data = d,
                None => {
                    // 解不开：可能是钥匙串里的密钥变了或文件损坏。旧文件改名留着，不让下一次保存把它覆盖掉。
                    let kept = path.with_extension(format!("bin.corrupt-{}", crate::db::now()));
                    match std::fs::rename(&path, &kept) {
                        Ok(()) => {
                            let name = kept.file_name().and_then(|n| n.to_str()).unwrap_or("vault.bin.corrupt").to_string();
                            log::error!("vault could not be decrypted, kept as {name}");
                            notice = Some(format!(
                                "密钥保险箱无法解密（密钥变了或文件损坏），旧文件已保留为 {name}。已保存的 API 密钥等需要重新填写；请再保存一次。"
                            ));
                        }
                        Err(e) => locked = locked.or(Some(format!("密钥保险箱无法解密，且无法保留旧文件：{e}"))),
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => locked = locked.or(Some(format!("无法读取密钥保险箱：{e}"))),
        }
        Vault { path: Some(path), key, data: RwLock::new(data), notice: Mutex::new(notice), locked }
    }

    /// 打开时发现旧保险箱解不开且已改名保留时的提示（尚未告知用户前一直有值）。
    pub fn recovery_notice(&self) -> Option<String> {
        self.notice.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn in_memory() -> Vault {
        Vault { path: None, key: [9; 32], data: RwLock::new(BTreeMap::new()), notice: Mutex::new(None), locked: None }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.data.read().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.data.read().unwrap_or_else(|e| e.into_inner()).get(name).is_some_and(|v| !v.is_empty())
    }

    pub fn set(&self, name: &str, value: &str) -> AppResult<()> {
        if !valid_name(name) {
            return Err(AppError::invalid("密钥名称不合法。"));
        }
        self.check_writable()?;
        {
            let mut d = self.data.write().unwrap_or_else(|e| e.into_inner());
            if value.is_empty() {
                d.remove(name);
            } else {
                d.insert(name.to_string(), value.to_string());
            }
        }
        self.save()
    }

    pub fn remove(&self, name: &str) -> AppResult<()> {
        self.set(name, "")
    }

    /// 删除所有密钥，名字以 `keep` 里某个前缀开头的保留（“一键清除”用，保留应用锁的密码记录）。
    pub fn clear_except(&self, keep: &[&str]) -> AppResult<()> {
        self.check_writable()?;
        self.data.write().unwrap_or_else(|e| e.into_inner()).retain(|k, _| keep.iter().any(|p| k.starts_with(p)));
        self.save()
    }

    /// 有锁定原因就拒绝；有“旧文件已保留”的提示就返回一次（本次不写入），告知用户后再保存即可。
    fn check_writable(&self) -> AppResult<()> {
        if let Some(why) = &self.locked {
            return Err(AppError::msg(why.clone()));
        }
        if let Some(msg) = self.notice.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Err(AppError::msg(msg));
        }
        Ok(())
    }

    fn save(&self) -> AppResult<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let plain = serde_json::to_vec(&*self.data.read().unwrap_or_else(|e| e.into_inner()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("bin.tmp");
        let r = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            secret::restrict_permissions(&tmp);
            f.write_all(&secret::encrypt(&self.key, &plain))?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = r {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_encrypted_on_disk_and_survive_reopen() {
        let dir = std::env::temp_dir().join(format!("clearclip-vault-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.bin");
        let key = [3u8; 32];
        let v = Vault::open(path.clone(), key);
        assert!(!v.has("ai.api_key"));
        v.set("ai.api_key", "sk-secret-123").unwrap();
        assert!(v.has("ai.api_key"));
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("sk-secret"));
        let again = Vault::open(path.clone(), key);
        assert_eq!(again.get("ai.api_key").as_deref(), Some("sk-secret-123"));
        again.remove("ai.api_key").unwrap();
        assert!(Vault::open(path, key).get("ai.api_key").is_none());
        assert!(v.set("Bad Name", "x").is_err());
        assert!(v.set("", "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clearclip-vault-{tag}-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn corrupt_files(dir: &std::path::Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.to_string_lossy().contains("corrupt")).collect()
    }

    #[test]
    fn undecryptable_vault_is_kept_and_first_set_reports_it() {
        let dir = tmpdir("bad");
        let path = dir.join("vault.bin");
        let v = Vault::open(path.clone(), [3u8; 32]);
        v.set("ai.api_key", "sk-old").unwrap();
        let original = std::fs::read(&path).unwrap();
        // 换了密钥：解不开
        let v2 = Vault::open(path.clone(), [4u8; 32]);
        assert!(v2.get("ai.api_key").is_none());
        assert!(v2.recovery_notice().is_some());
        let kept = corrupt_files(&dir);
        assert_eq!(kept.len(), 1, "旧文件改名保留");
        assert_eq!(std::fs::read(&kept[0]).unwrap(), original, "旧文件内容原样");
        assert!(!path.exists());
        let err = v2.set("ai.api_key", "sk-new").unwrap_err();
        assert!(err.message.contains("已保留"), "{}", err.message);
        assert!(!path.exists(), "出错的这次不写文件");
        // 提示过一次后可以正常保存，旧文件仍在
        v2.set("ai.api_key", "sk-new").unwrap();
        assert_eq!(Vault::open(path, [4u8; 32]).get("ai.api_key").as_deref(), Some("sk-new"));
        assert_eq!(std::fs::read(&corrupt_files(&dir)[0]).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_vault_file_is_kept_not_overwritten() {
        let dir = tmpdir("garbage");
        let path = dir.join("vault.bin");
        std::fs::write(&path, b"definitely not a vault").unwrap();
        let v = Vault::open(path.clone(), [1u8; 32]);
        assert!(v.set("a.b", "x").is_err());
        assert_eq!(std::fs::read(&corrupt_files(&dir)[0]).unwrap(), b"definitely not a vault");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locked_vault_refuses_every_write() {
        let dir = tmpdir("locked");
        let v = Vault::open_with(dir.join("vault.bin"), [1u8; 32], Some("密钥没能保存".into()));
        assert!(v.set("a.b", "x").is_err());
        assert!(v.clear_except(&[]).is_err());
        assert!(!dir.join("vault.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_except_keeps_the_app_lock_record() {
        let v = Vault::in_memory();
        v.set("applock.hash", "v1$64$1$1$00$00").unwrap();
        v.set("ai.api_key", "sk-1").unwrap();
        v.set("notify.tg", "token").unwrap();
        v.clear_except(&["applock."]).unwrap();
        assert!(v.has("applock.hash"));
        assert!(!v.has("ai.api_key") && !v.has("notify.tg"));
    }
}
