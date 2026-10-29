//! 加密保险箱：把敏感文件加密后放进 `safebox/` 目录，文件名和内容都看不出来。
//!
//! 密钥：随机的主密钥，用“密码经 Argon2id 派生的密钥”加密后保存在 `meta.json`。
//! 改密码只需要重新加密主密钥，不用动文件。每个文件用主密钥和文件编号派生独立的密钥，
//! 按 1 MiB 分块用 AES-256-GCM 加密：块序号放进 nonce，文件头和“是否最后一块”放进附加数据，
//! 所以截断、调换顺序、改文件头都会解密失败。文件名、大小等信息放在加密的 `index.bin` 里。
//!
//! 忘记密码无法找回，这是设计如此。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::security::Throttle;

pub const CHUNK: usize = 1 << 20;
const MAGIC: &[u8; 4] = b"CCV1";
const HEADER_LEN: usize = 12;
const TAG: usize = 16;
const AAD_MASTER: &[u8] = b"clearclip-safebox-master";
const AAD_INDEX: &[u8] = b"clearclip-safebox-index";
pub const MIN_PASSWORD: usize = 6;

/// Argon2id 参数。随 `meta.json` 一起保存，以后调整强度不影响已有的保险箱。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Kdf {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Kdf {
    /// 推荐值（19 MiB 内存、2 轮），一次校验约零点几秒。
    pub const STRONG: Kdf = Kdf { m_kib: 19_456, t: 2, p: 1 };
    /// 只给测试用
    #[cfg(test)]
    pub const LIGHT: Kdf = Kdf { m_kib: 64, t: 1, p: 1 };

    fn valid(&self) -> bool {
        (8..=262_144).contains(&self.m_kib) && (1..=10).contains(&self.t) && (1..=4).contains(&self.p)
    }
}

pub fn derive(password: &str, salt: &[u8], kdf: Kdf) -> AppResult<[u8; 32]> {
    if !kdf.valid() {
        return Err(AppError::msg("密钥参数无效，保险箱文件可能已损坏。"));
    }
    let params = argon2::Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|e| AppError::msg(format!("密钥参数无效：{e}")))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    a.hash_password_into(password.as_bytes(), salt, &mut out).map_err(|e| AppError::msg(format!("密钥派生失败：{e}")))?;
    Ok(out)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

fn cipher(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
}

fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let nonce: [u8; 12] = random_bytes();
    let mut out = nonce.to_vec();
    out.extend(cipher(key).encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad }).expect("aes-gcm encrypt"));
    out
}

fn open(key: &[u8; 32], aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 12 + TAG {
        return None;
    }
    let (nonce, ct) = data.split_at(12);
    cipher(key).decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad }).ok()
}

fn file_key(master: &[u8; 32], id: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"clearclip-safebox-file-v1");
    h.update(master);
    h.update(id.as_bytes());
    h.finalize().into()
}

fn chunk_nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    n
}

fn chunk_aad(header: &[u8; HEADER_LEN], last: bool) -> [u8; HEADER_LEN + 1] {
    let mut a = [0u8; HEADER_LEN + 1];
    a[..HEADER_LEN].copy_from_slice(header);
    a[HEADER_LEN] = last as u8;
    a
}

fn chunk_count(size: u64) -> u64 {
    size.div_ceil(CHUNK as u64).max(1)
}

fn locked_err() -> AppError {
    AppError::invalid("保险箱已锁定，请先输入密码解锁。")
}

fn corrupt() -> AppError {
    AppError::new(ErrorKind::Disk, "解密失败：文件已损坏，或不是这个保险箱里的文件。")
}

/// 加密文件。`progress(已处理, 总数)` 返回 false 表示取消。返回明文大小。
pub fn encrypt_file(master: &[u8; 32], id: &str, src: &Path, dst: &Path, progress: &mut dyn FnMut(u64, u64) -> bool) -> AppResult<u64> {
    let mut input = std::fs::File::open(src)?;
    let size = input.metadata()?.len();
    let c = cipher(&file_key(master, id));
    let mut out = std::io::BufWriter::new(std::fs::File::create(dst)?);
    let mut header = [0u8; HEADER_LEN];
    header[..4].copy_from_slice(MAGIC);
    header[4..].copy_from_slice(&size.to_le_bytes());
    out.write_all(&header)?;
    let chunks = chunk_count(size);
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    for n in 0..chunks {
        let len = (size - done).min(CHUNK as u64) as usize;
        input.read_exact(&mut buf[..len]).map_err(|_| AppError::new(ErrorKind::Disk, "文件在加密过程中发生了变化。"))?;
        let last = n + 1 == chunks;
        let ct = c
            .encrypt(Nonce::from_slice(&chunk_nonce(n)), Payload { msg: &buf[..len], aad: &chunk_aad(&header, last) })
            .map_err(|_| AppError::msg("加密失败"))?;
        out.write_all(&ct)?;
        done += len as u64;
        if !progress(done, size) {
            return Err(AppError::msg("canceled"));
        }
    }
    out.flush()?;
    out.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok(size)
}

/// 解密文件，返回明文大小。
pub fn decrypt_file(master: &[u8; 32], id: &str, src: &Path, dst: &Path, progress: &mut dyn FnMut(u64, u64) -> bool) -> AppResult<u64> {
    let mut input = std::fs::File::open(src)?;
    let total = input.metadata()?.len();
    let mut header = [0u8; HEADER_LEN];
    input.read_exact(&mut header).map_err(|_| corrupt())?;
    if &header[..4] != MAGIC {
        return Err(corrupt());
    }
    let size = u64::from_le_bytes(header[4..].try_into().unwrap());
    let chunks = chunk_count(size);
    if total != HEADER_LEN as u64 + size + chunks * TAG as u64 {
        return Err(corrupt());
    }
    let c = cipher(&file_key(master, id));
    let mut out = std::io::BufWriter::new(std::fs::File::create(dst)?);
    let mut buf = vec![0u8; CHUNK + TAG];
    let mut done = 0u64;
    for n in 0..chunks {
        let len = (size - done).min(CHUNK as u64) as usize + TAG;
        input.read_exact(&mut buf[..len]).map_err(|_| corrupt())?;
        let last = n + 1 == chunks;
        let plain = c.decrypt(Nonce::from_slice(&chunk_nonce(n)), Payload { msg: &buf[..len], aad: &chunk_aad(&header, last) }).map_err(|_| corrupt())?;
        out.write_all(&plain)?;
        done += plain.len() as u64;
        if !progress(done, size) {
            return Err(AppError::msg("canceled"));
        }
    }
    out.flush()?;
    out.into_inner().map_err(|e| AppError::from(e.into_error()))?.sync_all()?;
    Ok(size)
}

/// 先用零覆盖再删除。机械硬盘上有效；SSD、写时复制的文件系统不能保证覆盖到原位置，
/// 真正需要防恢复时请使用系统的整盘加密。
pub fn shred_file(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(path) {
            let zeros = vec![0u8; 1 << 16];
            let mut left = meta.len();
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                if f.write_all(&zeros[..n]).is_err() {
                    break;
                }
                left -= n as u64;
            }
            let _ = f.sync_all();
        }
    }
    let _ = std::fs::remove_file(path);
}

/// 先覆盖再删除整个文件夹。
pub fn shred_dir(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                shred_dir(&p);
            } else {
                shred_file(&p);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

pub fn kind_of(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "ts" | "m4v" | "wmv" => "video",
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" => "audio",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "avif" => "image",
        _ => "other",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeEntry {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub added_at: i64,
    pub kind: String,
    /// 来源：`file`（手动添加）或 `library`（从媒体库移入）
    pub from: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub count: usize,
    pub total_bytes: u64,
    pub locked_out_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    kdf: Kdf,
    salt: String,
    wrapped: String,
}

struct Unlocked {
    master: [u8; 32],
    entries: Vec<SafeEntry>,
}

impl Drop for Unlocked {
    fn drop(&mut self) {
        self.master.fill(0);
    }
}

pub struct Safebox {
    dir: PathBuf,
    state: Mutex<Option<Unlocked>>,
    last_used: Mutex<Instant>,
    pub throttle: Throttle,
}

impl Safebox {
    pub fn new(dir: PathBuf) -> Safebox {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            // 上次中途退出留下的半成品
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "part" || x == "tmp") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Safebox { dir, state: Mutex::new(None), last_used: Mutex::new(Instant::now()), throttle: Throttle::default() }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Option<Unlocked>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.bin")
    }

    pub fn blob_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.ccv"))
    }

    pub fn exists(&self) -> bool {
        self.meta_path().is_file()
    }

    pub fn is_unlocked(&self) -> bool {
        self.guard().is_some()
    }

    pub fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    pub fn idle_secs(&self) -> u64 {
        self.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed().as_secs()
    }

    pub fn status(&self) -> SafeStatus {
        let g = self.guard();
        let (count, total_bytes) = g.as_ref().map(|o| (o.entries.len(), o.entries.iter().map(|e| e.size).sum())).unwrap_or((0, 0));
        SafeStatus { exists: self.exists(), unlocked: g.is_some(), count, total_bytes, locked_out_secs: self.throttle.remaining_secs() }
    }

    fn wrap(&self, password: &str, master: &[u8; 32], kdf: Kdf) -> AppResult<Meta> {
        let salt: [u8; 16] = random_bytes();
        let kek = derive(password, &salt, kdf)?;
        Ok(Meta { version: 1, kdf, salt: hex::encode(salt), wrapped: hex::encode(seal(&kek, AAD_MASTER, master)) })
    }

    fn write_meta(&self, meta: &Meta) -> AppResult<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.meta_path().with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(meta)?)?;
        std::fs::rename(tmp, self.meta_path())?;
        Ok(())
    }

    fn read_meta(&self) -> AppResult<Meta> {
        let meta: Meta = serde_json::from_slice(&std::fs::read(self.meta_path())?).map_err(|_| AppError::msg("保险箱的 meta.json 已损坏。"))?;
        if meta.version != 1 {
            return Err(AppError::msg("这个保险箱来自更新的版本，请先升级清影。"));
        }
        Ok(meta)
    }

    /// `Ok(None)` 表示密码不对。
    fn unwrap_master(&self, password: &str) -> AppResult<Option<[u8; 32]>> {
        let meta = self.read_meta()?;
        let salt = hex::decode(&meta.salt).map_err(|_| AppError::msg("保险箱的 meta.json 已损坏。"))?;
        let wrapped = hex::decode(&meta.wrapped).map_err(|_| AppError::msg("保险箱的 meta.json 已损坏。"))?;
        let kek = derive(password, &salt, meta.kdf)?;
        Ok(open(&kek, AAD_MASTER, &wrapped).filter(|m| m.len() == 32).map(|m| m.try_into().unwrap()))
    }

    pub fn create(&self, password: &str, kdf: Kdf) -> AppResult<()> {
        if self.exists() {
            return Err(AppError::invalid("保险箱已经存在。"));
        }
        if password.chars().count() < MIN_PASSWORD {
            return Err(AppError::invalid(format!("密码至少 {MIN_PASSWORD} 个字符。")));
        }
        let master: [u8; 32] = random_bytes();
        self.write_meta(&self.wrap(password, &master, kdf)?)?;
        let open = Unlocked { master, entries: vec![] };
        self.save_index(&open)?;
        *self.guard() = Some(open);
        self.touch();
        Ok(())
    }

    pub fn unlock(&self, password: &str) -> AppResult<()> {
        if !self.exists() {
            return Err(AppError::not_found("还没有创建保险箱。"));
        }
        self.throttle.check()?;
        let Some(master) = self.unwrap_master(password)? else {
            self.throttle.fail();
            return Err(AppError::invalid("密码不对。"));
        };
        self.throttle.ok();
        let entries = self.load_index(&master)?;
        *self.guard() = Some(Unlocked { master, entries });
        self.touch();
        Ok(())
    }

    pub fn lock(&self) {
        *self.guard() = None;
    }

    pub fn change_password(&self, old: &str, new: &str, kdf: Kdf) -> AppResult<()> {
        if new.chars().count() < MIN_PASSWORD {
            return Err(AppError::invalid(format!("新密码至少 {MIN_PASSWORD} 个字符。")));
        }
        self.throttle.check()?;
        let Some(master) = self.unwrap_master(old)? else {
            self.throttle.fail();
            return Err(AppError::invalid("当前密码不对。"));
        };
        self.throttle.ok();
        self.write_meta(&self.wrap(new, &master, kdf)?)
    }

    fn load_index(&self, master: &[u8; 32]) -> AppResult<Vec<SafeEntry>> {
        for path in [self.index_path(), self.index_path().with_extension("bak")] {
            let Ok(data) = std::fs::read(&path) else { continue };
            if let Some(plain) = open(master, AAD_INDEX, &data) {
                if let Ok(list) = serde_json::from_slice(&plain) {
                    return Ok(list);
                }
            }
        }
        if self.index_path().exists() {
            return Err(AppError::new(ErrorKind::Disk, "保险箱的目录文件已损坏，无法读取文件列表。"));
        }
        Ok(vec![])
    }

    fn save_index(&self, open: &Unlocked) -> AppResult<()> {
        std::fs::create_dir_all(&self.dir)?;
        let data = seal(&open.master, AAD_INDEX, &serde_json::to_vec(&open.entries)?);
        let tmp = self.index_path().with_extension("tmp");
        std::fs::write(&tmp, data)?;
        if self.index_path().exists() {
            let _ = std::fs::copy(self.index_path(), self.index_path().with_extension("bak"));
        }
        std::fs::rename(tmp, self.index_path())?;
        Ok(())
    }

    fn master(&self) -> AppResult<[u8; 32]> {
        self.guard().as_ref().map(|o| o.master).ok_or_else(locked_err)
    }

    pub fn list(&self) -> AppResult<Vec<SafeEntry>> {
        let g = self.guard();
        let o = g.as_ref().ok_or_else(locked_err)?;
        self.touch();
        let mut v = o.entries.clone();
        v.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.name.cmp(&b.name)));
        Ok(v)
    }

    pub fn entry(&self, id: &str) -> AppResult<SafeEntry> {
        self.guard().as_ref().ok_or_else(locked_err)?.entries.iter().find(|e| e.id == id).cloned().ok_or_else(|| AppError::not_found("保险箱里没有这个文件"))
    }

    /// 加密一个文件放进保险箱（不删除原文件，由调用方决定）。
    pub fn add_file(&self, src: &Path, name: &str, from: &str, progress: &mut dyn FnMut(u64, u64) -> bool) -> AppResult<SafeEntry> {
        let master = self.master()?;
        std::fs::create_dir_all(&self.dir)?;
        let id = hex::encode(random_bytes::<12>());
        let part = self.dir.join(format!("{id}.ccv.part"));
        let size = match encrypt_file(&master, &id, src, &part, progress) {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
        };
        std::fs::rename(&part, self.blob_path(&id))?;
        let entry = SafeEntry { id, name: name.to_string(), size, added_at: crate::db::now(), kind: kind_of(name).into(), from: from.into() };
        let mut g = self.guard();
        let Some(o) = g.as_mut() else {
            // 加密期间被锁定：不留下找不到索引的孤立文件
            let _ = std::fs::remove_file(self.blob_path(&entry.id));
            return Err(locked_err());
        };
        o.entries.push(entry.clone());
        if let Err(e) = self.save_index(o) {
            o.entries.pop();
            let _ = std::fs::remove_file(self.blob_path(&entry.id));
            return Err(e);
        }
        self.touch();
        Ok(entry)
    }

    /// 解密到指定位置（先写 `.part` 再改名）。
    pub fn extract(&self, id: &str, dst: &Path, progress: &mut dyn FnMut(u64, u64) -> bool) -> AppResult<u64> {
        let master = self.master()?;
        self.entry(id)?;
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = dst.with_extension("part");
        match decrypt_file(&master, id, &self.blob_path(id), &part, progress) {
            Ok(n) => {
                std::fs::rename(&part, dst)?;
                self.touch();
                Ok(n)
            }
            Err(e) => {
                shred_file(&part);
                Err(e)
            }
        }
    }

    pub fn remove(&self, ids: &[String]) -> AppResult<usize> {
        let mut g = self.guard();
        let o = g.as_mut().ok_or_else(locked_err)?;
        let before = o.entries.len();
        o.entries.retain(|e| !ids.contains(&e.id));
        let removed = before - o.entries.len();
        self.save_index(o)?;
        for id in ids {
            if id.chars().all(|c| c.is_ascii_hexdigit()) {
                let _ = std::fs::remove_file(self.blob_path(id));
            }
        }
        self.touch();
        Ok(removed)
    }

    /// 销毁整个保险箱（不可恢复）。
    pub fn destroy(&self) {
        self.lock();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-safebox-{tag}-{}-{}", std::process::id(), crate::db::now()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn roundtrip(len: usize) {
        let dir = tmp("rt");
        let master = [7u8; 32];
        let src = dir.join("a.bin");
        let data = sample(len);
        std::fs::write(&src, &data).unwrap();
        let enc = dir.join("a.ccv");
        let n = encrypt_file(&master, "id1", &src, &enc, &mut |_, _| true).unwrap();
        assert_eq!(n as usize, len);
        let ct = std::fs::read(&enc).unwrap();
        assert_eq!(ct.len(), HEADER_LEN + len + chunk_count(len as u64) as usize * TAG);
        if len > 32 {
            assert!(!ct.windows(32).any(|w| w == &data[..32]), "plaintext must not appear in ciphertext");
        }
        let dec = dir.join("a.out");
        decrypt_file(&master, "id1", &enc, &dec, &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read(&dec).unwrap(), data);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn roundtrip_sizes_around_chunk_boundaries() {
        for len in [0, 1, 100, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 5] {
            roundtrip(len);
        }
    }

    #[test]
    fn tampering_truncation_and_wrong_key_are_rejected() {
        let dir = tmp("tamper");
        let master = [9u8; 32];
        let src = dir.join("a.bin");
        std::fs::write(&src, sample(CHUNK * 2 + 10)).unwrap();
        let enc = dir.join("a.ccv");
        encrypt_file(&master, "idx", &src, &enc, &mut |_, _| true).unwrap();
        let out = dir.join("o");
        // 密钥不对 / 文件编号不对
        assert!(decrypt_file(&[1u8; 32], "idx", &enc, &out, &mut |_, _| true).is_err());
        assert!(decrypt_file(&master, "other", &enc, &out, &mut |_, _| true).is_err());
        let good = std::fs::read(&enc).unwrap();
        // 改一个字节
        let mut bad = good.clone();
        bad[HEADER_LEN + 100] ^= 1;
        std::fs::write(&enc, &bad).unwrap();
        assert!(decrypt_file(&master, "idx", &enc, &out, &mut |_, _| true).is_err());
        // 截断：丢掉最后一块
        std::fs::write(&enc, &good[..good.len() - (10 + TAG)]).unwrap();
        assert!(decrypt_file(&master, "idx", &enc, &out, &mut |_, _| true).is_err());
        // 改文件头里的大小
        let mut bad = good.clone();
        bad[4] ^= 1;
        std::fs::write(&enc, &bad).unwrap();
        assert!(decrypt_file(&master, "idx", &enc, &out, &mut |_, _| true).is_err());
        // 调换两个整块
        let mut swapped = good.clone();
        let (a, b) = (HEADER_LEN, HEADER_LEN + CHUNK + TAG);
        let first: Vec<u8> = swapped[a..a + CHUNK + TAG].to_vec();
        let second: Vec<u8> = swapped[b..b + CHUNK + TAG].to_vec();
        swapped[a..a + CHUNK + TAG].copy_from_slice(&second);
        swapped[b..b + CHUNK + TAG].copy_from_slice(&first);
        std::fs::write(&enc, &swapped).unwrap();
        assert!(decrypt_file(&master, "idx", &enc, &out, &mut |_, _| true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancel_stops_encryption() {
        let dir = tmp("cancel");
        let src = dir.join("a.bin");
        std::fs::write(&src, sample(CHUNK * 3)).unwrap();
        let mut calls = 0;
        let err = encrypt_file(&[1u8; 32], "i", &src, &dir.join("o"), &mut |_, _| {
            calls += 1;
            calls < 2
        })
        .unwrap_err();
        assert_eq!(err.message, "canceled");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_unlock_add_extract_remove_and_password_change() {
        let dir = tmp("box");
        let sb = Safebox::new(dir.join("safebox"));
        assert!(!sb.exists());
        assert!(sb.create("short", Kdf::LIGHT).is_err(), "too short");
        sb.create("correct horse", Kdf::LIGHT).unwrap();
        assert!(sb.exists() && sb.is_unlocked());
        assert!(sb.create("another one", Kdf::LIGHT).is_err());

        let src = dir.join("私密 视频.mp4");
        let data = sample(CHUNK + 123);
        std::fs::write(&src, &data).unwrap();
        let e = sb.add_file(&src, "私密 视频.mp4", "file", &mut |_, _| true).unwrap();
        assert_eq!((e.kind.as_str(), e.size as usize), ("video", data.len()));
        assert_eq!(sb.list().unwrap().len(), 1);
        // 磁盘上看不到文件名，也看不到明文
        let mut names = vec![];
        for f in std::fs::read_dir(dir.join("safebox")).unwrap().flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            let raw = std::fs::read(f.path()).unwrap();
            assert!(!String::from_utf8_lossy(&raw).contains("私密"), "{n} leaks the file name");
            names.push(n);
        }
        assert!(names.iter().any(|n| n.ends_with(".ccv")) && names.contains(&"index.bin".to_string()));

        let out = dir.join("restored.mp4");
        sb.extract(&e.id, &out, &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), data);

        // 锁定后不能读
        sb.lock();
        assert!(sb.list().is_err());
        assert!(sb.extract(&e.id, &dir.join("x"), &mut |_, _| true).is_err());
        assert!(sb.unlock("wrong password").is_err());
        sb.unlock("correct horse").unwrap();
        assert_eq!(sb.list().unwrap()[0].name, "私密 视频.mp4");

        // 重新打开（模拟重启）仍然可用
        let sb2 = Safebox::new(dir.join("safebox"));
        assert!(sb2.exists() && !sb2.is_unlocked());
        sb2.unlock("correct horse").unwrap();
        assert_eq!(sb2.list().unwrap().len(), 1);

        // 改密码：旧密码失效，文件不用重新加密
        assert!(sb2.change_password("nope nope", "new password", Kdf::LIGHT).is_err());
        sb2.change_password("correct horse", "new password", Kdf::LIGHT).unwrap();
        sb2.lock();
        assert!(sb2.unlock("correct horse").is_err());
        sb2.unlock("new password").unwrap();
        let out2 = dir.join("again.mp4");
        sb2.extract(&e.id, &out2, &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read(&out2).unwrap(), data);

        assert_eq!(sb2.remove(std::slice::from_ref(&e.id)).unwrap(), 1);
        assert!(sb2.list().unwrap().is_empty());
        assert!(!sb2.blob_path(&e.id).exists());
        sb2.destroy();
        assert!(!sb2.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn repeated_wrong_passwords_lock_out_even_the_right_one() {
        let dir = tmp("throttle");
        let sb = Safebox::new(dir.join("safebox"));
        sb.create("correct horse", Kdf::LIGHT).unwrap();
        sb.lock();
        for _ in 0..5 {
            assert!(sb.unlock("bad password").is_err());
        }
        let err = sb.unlock("correct horse").unwrap_err();
        assert!(err.message.contains("秒"), "{}", err.message);
        assert!(sb.status().locked_out_secs > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupted_index_falls_back_to_backup() {
        let dir = tmp("bak");
        let sb = Safebox::new(dir.join("safebox"));
        sb.create("correct horse", Kdf::LIGHT).unwrap();
        let src = dir.join("a.txt");
        std::fs::write(&src, b"hello").unwrap();
        sb.add_file(&src, "a.txt", "file", &mut |_, _| true).unwrap();
        sb.add_file(&src, "b.txt", "file", &mut |_, _| true).unwrap();
        sb.lock();
        std::fs::write(dir.join("safebox").join("index.bin"), b"garbage garbage garbage garbage").unwrap();
        sb.unlock("correct horse").unwrap();
        // 备份是倒数第二次保存的内容：只有 a.txt
        assert_eq!(sb.list().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shred_removes_file_and_dir() {
        let dir = tmp("shred");
        let f = dir.join("sub").join("x.bin");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, sample(5000)).unwrap();
        shred_file(&f);
        assert!(!f.exists());
        std::fs::write(&f, b"1").unwrap();
        shred_dir(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of("a.MP4"), "video");
        assert_eq!(kind_of("a.flac"), "audio");
        assert_eq!(kind_of("a.jpeg"), "image");
        assert_eq!(kind_of("noext"), "other");
    }
}
