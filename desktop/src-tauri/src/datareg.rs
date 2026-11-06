//! 数据清单：数据目录里每样东西是什么——用户自己积累的（进备份）、可以重新生成的缓存、还是机密（不进备份）。
//! 新模块往数据目录里写东西时，在这里登记一行，备份和“清理缓存”就都知道它了，不会再漏。

use std::path::{Path, PathBuf};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// 用户自己积累的：生词、术语、专家、摘录、原声……进备份
    User,
    /// 可以重新生成的缓存：不进备份，可以清理
    Cache,
    /// 密钥等机密：不进备份
    Secret,
}

#[derive(Debug, Clone, Copy)]
pub struct Item {
    /// 相对数据目录的路径
    pub path: &'static str,
    pub class: Class,
    /// 是目录（整个目录进备份）
    pub dir: bool,
    /// SQLite 数据库：备份时用 `VACUUM INTO` 拍快照，不直接复制正在写的文件
    pub sqlite: bool,
    pub label: &'static str,
}

const fn user(path: &'static str, label: &'static str) -> Item {
    Item { path, class: Class::User, dir: false, sqlite: false, label }
}

/// 数据目录里除了主数据库（clearclip.db，备份单独处理）以外的东西。
pub const ITEMS: &[Item] = &[
    Item { path: "vocab.db", class: Class::User, dir: false, sqlite: true, label: "生词本和复习记录" },
    Item { path: "vocab-clips", class: Class::User, dir: true, sqlite: false, label: "生词原声" },
    user("glossaries.json", "术语库"),
    user("experts.json", "AI 专家"),
    user("excerpts.json", "摘录"),
    Item { path: "tm.db", class: Class::User, dir: false, sqlite: true, label: "翻译记忆（含人工改过的译文）" },
    Item { path: "videocache.db", class: Class::Cache, dir: false, sqlite: true, label: "视频缓存（摘要会话、双语字幕）" },
    Item { path: "filetr", class: Class::Cache, dir: true, sqlite: false, label: "文件翻译缓存" },
    Item { path: "vsum-cache", class: Class::Cache, dir: true, sqlite: false, label: "在线字幕 / 转写缓存" },
    Item { path: "transcripts", class: Class::Cache, dir: true, sqlite: false, label: "转写文稿" },
    Item { path: "previews", class: Class::Cache, dir: true, sqlite: false, label: "预览图" },
    Item { path: "vault.bin", class: Class::Secret, dir: false, sqlite: false, label: "保存的密钥" },
];

/// 备份里放用户数据的目录前缀。
pub const BACKUP_PREFIX: &str = "data/";

/// 一个用户数据文件：备份里的名字（相对数据目录，用 `/`）和本机路径。
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub rel: String,
    pub path: PathBuf,
    pub sqlite: bool,
}

/// 现在数据目录里有的用户数据文件（目录展开成里面的文件；只展开一层，用户数据目录都是平的）。
pub fn user_files(data_dir: &Path) -> Vec<File> {
    let mut out = vec![];
    for it in ITEMS.iter().filter(|i| i.class == Class::User) {
        let p = data_dir.join(it.path);
        if it.dir {
            let Ok(rd) = std::fs::read_dir(&p) else { continue };
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    out.push(File { rel: format!("{}/{}", it.path, e.file_name().to_string_lossy()), path: e.path(), sqlite: false });
                }
            }
        } else if p.is_file() {
            out.push(File { rel: it.path.to_string(), path: p, sqlite: it.sqlite });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// 备份里的一个名字能不能还原到数据目录：只认清单里登记过的用户数据，防止压缩包里的路径往外跳。
pub fn restorable(rel: &str) -> bool {
    if rel.contains("..") || rel.contains('\\') || rel.starts_with('/') || rel.is_empty() {
        return false;
    }
    ITEMS.iter().filter(|i| i.class == Class::User).any(|i| {
        if i.dir {
            rel.strip_prefix(i.path).and_then(|r| r.strip_prefix('/')).is_some_and(|name| !name.is_empty() && !name.contains('/'))
        } else {
            rel == i.path
        }
    })
}

/// 读出一个用户数据文件的内容；SQLite 数据库先拍一份一致的快照。
pub fn read_for_backup(f: &File, tmp_dir: &Path) -> AppResult<Vec<u8>> {
    if !f.sqlite {
        return Ok(std::fs::read(&f.path)?);
    }
    std::fs::create_dir_all(tmp_dir)?;
    let snap = tmp_dir.join(format!("{}.snap", f.rel.replace('/', "_")));
    let _ = std::fs::remove_file(&snap);
    let r = (|| -> AppResult<Vec<u8>> {
        let c = rusqlite::Connection::open(&f.path).map_err(|e| crate::error::AppError::msg(e.to_string()))?;
        c.execute("VACUUM INTO ?1", [snap.to_string_lossy().as_ref()]).map_err(|e| crate::error::AppError::msg(e.to_string()))?;
        Ok(std::fs::read(&snap)?)
    })();
    let _ = std::fs::remove_file(&snap);
    r
}

/// 清掉所有缓存（“设置 → 清理缓存”）。返回清掉了几项。
pub fn clear_caches(data_dir: &Path) -> usize {
    let mut n = 0;
    for it in ITEMS.iter().filter(|i| i.class == Class::Cache && !i.sqlite) {
        let p = data_dir.join(it.path);
        if it.dir && p.is_dir() && std::fs::remove_dir_all(&p).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registered_user_data_is_backed_up_and_restorable() {
        let d = std::env::temp_dir().join(format!("cc-datareg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("vocab-clips")).unwrap();
        std::fs::create_dir_all(d.join("filetr")).unwrap();
        std::fs::write(d.join("glossaries.json"), "{}").unwrap();
        std::fs::write(d.join("experts.json"), "{}").unwrap();
        std::fs::write(d.join("vocab-clips/clearclip_hello.webm"), "x").unwrap();
        std::fs::write(d.join("filetr/abc.json"), "{}").unwrap(); // 缓存：不进备份
        std::fs::write(d.join("vault.bin"), "secret").unwrap(); // 机密：不进备份
        let rels: Vec<String> = user_files(&d).into_iter().map(|f| f.rel).collect();
        assert_eq!(rels, ["experts.json", "glossaries.json", "vocab-clips/clearclip_hello.webm"]);
        assert!(restorable("vocab.db") && restorable("vocab-clips/a.webm") && restorable("excerpts.json"));
        assert!(!restorable("vault.bin") && !restorable("../evil") && !restorable("vocab-clips/../x") && !restorable("filetr/a.json"));
        assert!(!restorable("vocab-clips/") && !restorable("vocab-clips/a/b"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sqlite_files_are_snapshotted_not_copied_raw() {
        let d = std::env::temp_dir().join(format!("cc-datareg-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let c = rusqlite::Connection::open(d.join("vocab.db")).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE w(x TEXT); INSERT INTO w VALUES('hello');").unwrap();
        let f = user_files(&d).into_iter().find(|f| f.rel == "vocab.db").unwrap();
        let bytes = read_for_backup(&f, &d.join("tmp")).unwrap();
        let out = d.join("copy.db");
        std::fs::write(&out, bytes).unwrap();
        let n: String = rusqlite::Connection::open(&out).unwrap().query_row("SELECT x FROM w", [], |r| r.get(0)).unwrap();
        assert_eq!(n, "hello", "WAL 里还没合并的内容也在快照里");
        drop(c);
        let _ = std::fs::remove_dir_all(&d);
    }
}
