//! 备份与还原：把数据库（媒体库记录、标签、订阅、直播间、历史、收件箱）和设置打成一个 zip。
//! 不包含登录 Cookie、密钥和手机配对令牌（换电脑后需要重新登录、重新配对）。
//!
//! 还原分两步：先把备份解压到 `restore-pending/`，下次启动时（数据库打开之前）再替换，
//! 这样不用在运行中替换正在使用的数据库文件。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};

const MANIFEST: &str = "manifest.json";
const DB_NAME: &str = "clearclip.db";
const SETTINGS_NAME: &str = "settings.json";
const KIND: &str = "clearclip-backup";
const PENDING: &str = "restore-pending";

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    kind: String,
    format: u32,
    app_version: String,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub app_version: String,
    pub created_at: i64,
    pub size: u64,
}

/// 设置里不放进备份的敏感字段。
fn scrub_settings(mut v: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("cookies");
        obj.remove("cookieUpdatedAt");
        if let Some(phone) = obj.get_mut("phone").and_then(|p| p.as_object_mut()) {
            phone.insert("token".into(), "".into());
            phone.insert("devices".into(), serde_json::json!([]));
            phone.insert("enabled".into(), false.into());
        }
    }
    v
}

pub fn export(db: &Db, settings: &crate::settings::Settings, app_version: &str, dest: &Path) -> AppResult<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let snap = dest.with_extension("db.tmp");
    db.snapshot_to(&snap)?;
    let result = (|| -> AppResult<()> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dest)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let m = Manifest { kind: KIND.into(), format: 1, app_version: app_version.into(), created_at: crate::db::now() };
        zip.start_file(MANIFEST, opts).map_err(|e| AppError::msg(format!("写入备份失败：{e}")))?;
        zip.write_all(&serde_json::to_vec_pretty(&m)?)?;
        zip.start_file(SETTINGS_NAME, opts).map_err(|e| AppError::msg(format!("写入备份失败：{e}")))?;
        zip.write_all(&serde_json::to_vec_pretty(&scrub_settings(serde_json::to_value(settings)?))?)?;
        zip.start_file(DB_NAME, opts).map_err(|e| AppError::msg(format!("写入备份失败：{e}")))?;
        let mut f = std::fs::File::open(&snap)?;
        std::io::copy(&mut f, &mut zip)?;
        zip.finish().map_err(|e| AppError::msg(format!("写入备份失败：{e}")))?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&snap);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64) -> AppResult<Vec<u8>> {
    let f = zip.by_name(name).map_err(|_| AppError::invalid(format!("这不是清影的备份文件（缺少 {name}）。")))?;
    let mut buf = vec![];
    f.take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

/// 检查备份文件并解压到待还原目录，重启后生效。
pub fn stage_import(src: &Path, data_dir: &Path) -> AppResult<BackupInfo> {
    let file = std::fs::File::open(src).map_err(|e| AppError::invalid(format!("无法打开备份文件：{e}")))?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut zip = zip::ZipArchive::new(file).map_err(|_| AppError::invalid("这不是清影的备份文件。"))?;
    let m: Manifest = serde_json::from_slice(&read_entry(&mut zip, MANIFEST, 64 * 1024)?).map_err(|_| AppError::invalid("备份文件的说明已损坏。"))?;
    if m.kind != KIND || m.format != 1 {
        return Err(AppError::invalid("这个备份来自不兼容的版本。"));
    }
    let settings = read_entry(&mut zip, SETTINGS_NAME, 4 * 1024 * 1024)?;
    serde_json::from_slice::<serde_json::Value>(&settings).map_err(|_| AppError::invalid("备份里的设置已损坏。"))?;
    let pending = data_dir.join(PENDING);
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(&pending)?;
    let write = (|| -> AppResult<()> {
        std::fs::write(pending.join(SETTINGS_NAME), &settings)?;
        let mut db_entry = zip.by_name(DB_NAME).map_err(|_| AppError::invalid("备份里没有数据库。"))?;
        let mut out = std::fs::File::create(pending.join(DB_NAME))?;
        std::io::copy(&mut db_entry, &mut out)?;
        Ok(())
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(e);
    }
    // 数据库必须能打开且有媒体库表
    let check = Db::open(&pending.join(DB_NAME)).and_then(|d| d.library_stats().map(|_| ()));
    if let Err(e) = check {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(AppError::invalid(format!("备份里的数据库无法使用：{e}")));
    }
    Ok(BackupInfo { app_version: m.app_version, created_at: m.created_at, size })
}

pub fn has_pending(data_dir: &Path) -> bool {
    data_dir.join(PENDING).join(DB_NAME).exists()
}

/// 启动时调用（数据库打开之前）：有待还原的备份就替换当前数据库和设置，原文件保留为 `.before-restore`。
pub fn apply_pending(data_dir: &Path, settings_path: &Path) -> bool {
    let pending = data_dir.join(PENDING);
    let new_db = pending.join(DB_NAME);
    if !new_db.exists() {
        return false;
    }
    let db_path = data_dir.join(DB_NAME);
    let keep = |p: &Path| -> PathBuf { p.with_file_name(format!("{}.before-restore", p.file_name().and_then(|n| n.to_str()).unwrap_or("file"))) };
    let result = (|| -> std::io::Result<()> {
        if db_path.exists() {
            std::fs::rename(&db_path, keep(&db_path))?;
        }
        // 旧数据库的 WAL / SHM 文件不能留给新数据库
        for ext in ["db-wal", "db-shm"] {
            let _ = std::fs::remove_file(db_path.with_extension(ext));
        }
        std::fs::rename(&new_db, &db_path).or_else(|_| std::fs::copy(&new_db, &db_path).map(|_| ()))?;
        if let Ok(bytes) = std::fs::read(pending.join(SETTINGS_NAME)) {
            if let Ok(mut incoming) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                // 手机配对、临时文件目录等和本机有关的内容保留当前的
                let current: Option<serde_json::Value> = std::fs::read(settings_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if let (Some(cur), Some(inc)) = (current.as_ref().and_then(|c| c.as_object()), incoming.as_object_mut()) {
                    for key in ["phone", "disclaimerAccepted", "downloadDir", "tempDir"] {
                        match cur.get(key) {
                            Some(v) => {
                                inc.insert(key.to_string(), v.clone());
                            }
                            None => {
                                inc.remove(key);
                            }
                        }
                    }
                }
                if settings_path.exists() {
                    let _ = std::fs::copy(settings_path, keep(settings_path));
                }
                std::fs::write(settings_path, serde_json::to_vec_pretty(&incoming).unwrap_or(bytes))?;
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&pending);
    match result {
        Ok(()) => true,
        Err(e) => {
            log::error!("restoring backup failed: {e}");
            // 尽量恢复原来的数据库
            if !db_path.exists() && keep(&db_path).exists() {
                let _ = std::fs::rename(keep(&db_path), &db_path);
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-bk-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn add(db: &Db, title: &str) -> i64 {
        db.record_download(&crate::db::NewDownload {
            platform: "x",
            media_id: title,
            asset_id: "video",
            title,
            author: "a",
            cover: None,
            path: "/nonexistent/f.mp4",
            size: 5,
            kind: "video",
            source: "manual",
            source_url: "",
            platform_name: "X",
        })
        .unwrap();
        db.id_by_key("x", title, "video").unwrap().unwrap()
    }

    #[test]
    fn export_stage_and_apply_roundtrip() {
        let dir = temp("rt");
        // 原数据
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let db = Db::open(&data.join(DB_NAME)).unwrap();
        let id = add(&db, "课程");
        db.set_item_tags(id, &["学习".into(), "重要".into()]).unwrap();
        db.set_item_meta(id, Some(true), Some(4), Some("备注")).unwrap();
        let settings = Settings {
            concurrency: 5,
            download_dir: "/old/dir".into(),
            phone: crate::settings::PhoneSettings { token: "SECRET".into(), enabled: true, ..Default::default() },
            ..Default::default()
        };
        let zip = dir.join("b.zip");
        export(&db, &settings, "9.9.9", &zip).unwrap();
        // 备份里没有敏感字段
        let mut z = zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
        let text = String::from_utf8(read_entry(&mut z, SETTINGS_NAME, 1 << 20).unwrap()).unwrap();
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.contains("\"concurrency\": 5"));

        // 在另一台“电脑”上还原
        let data2 = dir.join("data2");
        std::fs::create_dir_all(&data2).unwrap();
        let settings_path = dir.join("cfg/settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let local = Settings {
            download_dir: "/new/dir".into(),
            phone: crate::settings::PhoneSettings { token: "LOCALTOKEN".into(), ..Default::default() },
            ..Default::default()
        };
        local.save(&settings_path).unwrap();
        {
            let other = Db::open(&data2.join(DB_NAME)).unwrap();
            add(&other, "旧数据");
        }
        let info = stage_import(&zip, &data2).unwrap();
        assert_eq!(info.app_version, "9.9.9");
        assert!(has_pending(&data2));
        assert!(apply_pending(&data2, &settings_path));
        assert!(!has_pending(&data2));
        let restored = Db::open(&data2.join(DB_NAME)).unwrap();
        let items = restored.search_library(&crate::db::LibraryFilter::default(), 100).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].title.as_str(), items[0].favorite, items[0].rating, items[0].note.as_str()), ("课程", true, 4, "备注"));
        assert_eq!(items[0].tags.len(), 2);
        assert!(data2.join("clearclip.db.before-restore").exists());
        let s = Settings::load(&settings_path, Path::new("/dl"));
        assert_eq!(s.concurrency, 5, "settings come from the backup");
        assert_eq!(s.download_dir, "/new/dir", "machine specific path stays");
        assert_eq!(s.phone.token, "LOCALTOKEN", "pairing token stays local");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_foreign_files() {
        let dir = temp("bad");
        let not_zip = dir.join("x.zip");
        std::fs::write(&not_zip, b"hello").unwrap();
        assert!(stage_import(&not_zip, &dir).is_err());
        // 是 zip 但不是备份
        let other = dir.join("o.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
            z.start_file("a.txt", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        assert!(stage_import(&other, &dir).unwrap_err().to_string().contains("不是清影的备份"));
        assert!(!has_pending(&dir));
        assert!(!apply_pending(&dir, &dir.join("s.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
