//! 密钥保险箱：API 密钥、WebDAV 密码、通知令牌等敏感信息加密保存在 `vault.bin`，
//! 不写进 settings.json，也不进入备份。密钥与 Cookie 存储共用系统钥匙串里的那一把。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::secret;

pub struct Vault {
    path: Option<PathBuf>,
    key: [u8; 32],
    data: RwLock<BTreeMap<String, String>>,
}

/// 名称只能用小写字母、数字和 `. _ -`，防止被拿来读写别的东西。
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

impl Vault {
    pub fn open(path: PathBuf, key: [u8; 32]) -> Vault {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|bytes| secret::decrypt(&key, &bytes))
            .and_then(|plain| serde_json::from_slice::<BTreeMap<String, String>>(&plain).ok())
            .unwrap_or_default();
        Vault { path: Some(path), key, data: RwLock::new(data) }
    }

    pub fn in_memory() -> Vault {
        Vault { path: None, key: [9; 32], data: RwLock::new(BTreeMap::new()) }
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
        self.data.write().unwrap_or_else(|e| e.into_inner()).retain(|k, _| keep.iter().any(|p| k.starts_with(p)));
        self.save()
    }

    fn save(&self) -> AppResult<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let plain = serde_json::to_vec(&*self.data.read().unwrap_or_else(|e| e.into_inner()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("bin.tmp");
        std::fs::write(&tmp, secret::encrypt(&self.key, &plain))?;
        secret::restrict_permissions(&tmp);
        std::fs::rename(tmp, path)?;
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
        // 密钥不对读不出来（不报错，当作空）
        assert!(Vault::open(path.clone(), [4u8; 32]).get("ai.api_key").is_none());
        again.remove("ai.api_key").unwrap();
        assert!(Vault::open(path, key).get("ai.api_key").is_none());
        assert!(v.set("Bad Name", "x").is_err());
        assert!(v.set("", "x").is_err());
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
