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
}

pub fn load_or_create_key(data_dir: &Path) -> KeyInfo {
    match keyring_key() {
        Ok(key) => KeyInfo { key, in_keyring: true },
        Err(e) => {
            log::warn!("system keyring unavailable, falling back to key file: {e}");
            KeyInfo { key: file_key(data_dir), in_keyring: false }
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

fn file_key(data_dir: &Path) -> [u8; 32] {
    let path = data_dir.join("cookie.key");
    if let Some(key) = std::fs::read_to_string(&path).ok().and_then(|s| decode_key(s.trim())) {
        return key;
    }
    let key = new_key();
    let _ = std::fs::create_dir_all(data_dir);
    if std::fs::write(&path, hex::encode(key)).is_ok() {
        restrict_permissions(&path);
    }
    key
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
        let a = file_key(&dir);
        let b = file_key(&dir);
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(dir);
    }
}
