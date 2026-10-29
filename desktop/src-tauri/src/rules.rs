//! 自动规则：下载完成后，按条件（平台、作者、标题关键词、类型、来源、大小）自动打标签、收藏、
//! 移动到子目录、提取音频、视频规整、上传、推送通知。

use std::path::Path;
use std::sync::Arc;

use regex::Regex;

use crate::db::LibraryItem;
use crate::library::{self, MovePlan};
use crate::settings::{AutoRule, RuleWhen};
use crate::AppState;

/// 文字条件：`re:` 开头是正则；否则 `|` 分隔的任意关键词出现即可（不区分大小写）。
pub fn text_match(pattern: &str, text: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return true;
    }
    if let Some(re) = p.strip_prefix("re:") {
        return Regex::new(re).map(|r| r.is_match(text)).unwrap_or(false);
    }
    let t = text.to_lowercase();
    p.split('|').map(str::trim).filter(|k| !k.is_empty()).any(|k| t.contains(&k.to_lowercase()))
}

impl RuleWhen {
    pub fn matches(&self, item: &LibraryItem) -> bool {
        let platform = self.platform.trim();
        (platform.is_empty() || platform.eq_ignore_ascii_case(&item.platform) || platform == item.platform_name)
            && text_match(&self.author, &item.author)
            && text_match(&self.title, &item.title)
            && (self.kind.is_empty() || self.kind == item.kind)
            && (self.source.is_empty() || self.source == item.source)
            && (self.min_size_mb == 0 || item.size >= (self.min_size_mb * 1024 * 1024) as i64)
    }
}

/// 命中的规则（保持设置里的顺序）。
pub fn matching<'a>(rules: &'a [AutoRule], item: &LibraryItem) -> Vec<&'a AutoRule> {
    rules.iter().filter(|r| r.enabled && r.when.matches(item)).collect()
}

/// 只改数据库和文件位置的动作：标签、收藏、移动。返回新的文件路径（移动后）和说明。
pub fn apply_local(db: &crate::db::Db, root: &Path, item: &LibraryItem, rule: &AutoRule) -> (String, Vec<String>) {
    let mut notes = vec![];
    let mut path = item.path.clone();
    if !rule.then.add_tags.is_empty() && db.bulk_tags(&[item.id], &rule.then.add_tags, false).is_ok() {
        notes.push(format!("标签 {}", rule.then.add_tags.join("、")));
    }
    if rule.then.favorite && db.set_item_meta(item.id, Some(true), None, None).is_ok() {
        notes.push("收藏".into());
    }
    if !rule.then.move_to.is_empty() {
        let dir = root.join(library::render_dir(&rule.then.move_to, item));
        if let Some(name) = Path::new(&item.path).file_name() {
            let to = dir.join(name);
            if to.to_string_lossy() != item.path {
                let plan = MovePlan { id: item.id, from: item.path.clone(), to: to.to_string_lossy().into_owned() };
                if let Ok(rep) = library::apply_reorganize(db, &[plan]) {
                    if rep.moved == 1 {
                        path = to.to_string_lossy().into_owned();
                        notes.push(format!("移到 {}", dir.display()));
                    }
                }
            }
        }
    }
    (path, notes)
}

/// 下载完成后执行所有命中的规则。
pub fn run_for(app: &tauri::AppHandle, st: &Arc<AppState>, item: &LibraryItem) {
    let settings = st.settings();
    if matches!(item.source.as_str(), "tool" | "ai") {
        return;
    }
    let rules = matching(&settings.rules, item);
    if rules.is_empty() {
        return;
    }
    let root = settings.download_root();
    for rule in rules {
        let (path, mut notes) = apply_local(&st.db, &root, item, rule);
        if !rule.then.extract_audio.is_empty() && matches!(item.kind.as_str(), "video") && Path::new(&path).is_file() {
            let job = crate::media_tools::ToolJob {
                inputs: vec![path.clone()],
                op: crate::media_tools::ToolOp::Convert { format: rule.then.extract_audio.clone(), reencode: false },
                output_dir: None,
            };
            if crate::media_tools::start(app, job).is_ok() {
                notes.push(format!("提取 {} 音频", rule.then.extract_audio.to_uppercase()));
            }
        }
        if let Some(spec) = crate::vidnorm::spec::preset(&rule.then.normalize) {
            if matches!(item.kind.as_str(), "video") && Path::new(&path).is_file() {
                let job = crate::media_tools::ToolJob {
                    inputs: vec![path.clone()],
                    op: crate::media_tools::ToolOp::Normalize { spec, preset: Some(rule.then.normalize.clone()) },
                    output_dir: None,
                };
                if crate::media_tools::start(app, job).is_ok() {
                    notes.push("视频规整".into());
                }
            }
        }
        if rule.then.upload && settings.upload.url.is_empty() {
            log::warn!("rule {} wants to upload but no upload target is configured", rule.name);
        } else if rule.then.upload {
            let (id, title) = (item.id, item.title.clone());
            crate::media_tools::spawn_job(app, title, "上传", move |ctx| async move { crate::upload::run_upload(&ctx, id).await });
            notes.push("上传".into());
        }
        if rule.then.notify {
            crate::notify::emit(app, crate::notify::Event::Rule, &format!("规则“{}”已处理", rule.name), &format!("{}\n{}", item.title, notes.join("，")));
        }
        log::info!("rule {} matched {}: {}", rule.name, item.title, notes.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::RuleThen;

    fn item() -> LibraryItem {
        LibraryItem {
            id: 1,
            platform: "bilibili".into(),
            media_id: "BV1".into(),
            asset_id: "video".into(),
            title: "【教程】Rust 入门第 3 课".into(),
            author: "码农老张".into(),
            cover: None,
            path: "/dl/a.mp4".into(),
            size: 50 * 1024 * 1024,
            finished_at: 1_700_000_000,
            exists: true,
            kind: "video".into(),
            source: "subscription".into(),
            source_url: String::new(),
            platform_name: "B站".into(),
            cover_path: None,
            favorite: false,
            rating: 0,
            note: String::new(),
            tags: vec![],
            duration_ms: None,
        }
    }

    #[test]
    fn text_conditions() {
        assert!(text_match("", "anything"));
        assert!(text_match("教程|入门", "Rust 入门"));
        assert!(text_match("RUST", "rust book"), "case-insensitive");
        assert!(!text_match("教程|入门", "旅行 vlog"));
        assert!(text_match(r"re:第\s*\d+\s*课", "第 3 课"));
        assert!(!text_match("re:(", "x"), "bad regex never matches");
        assert!(text_match(" a | ", "a"), "blank keywords are ignored");
        assert!(!text_match("|", "a"), "only blanks match nothing");
    }

    #[test]
    fn conditions_all_have_to_hold() {
        let it = item();
        let w = |f: &dyn Fn(&mut RuleWhen)| {
            let mut w = RuleWhen::default();
            f(&mut w);
            w.matches(&it)
        };
        assert!(w(&|_| {}), "no conditions: always");
        assert!(w(&|w| w.platform = "bilibili".into()));
        assert!(w(&|w| w.platform = "B站".into()), "platform name works too");
        assert!(!w(&|w| w.platform = "douyin".into()));
        assert!(w(&|w| w.author = "老张".into()));
        assert!(w(&|w| w.kind = "video".into()) && !w(&|w| w.kind = "audio".into()));
        assert!(w(&|w| w.source = "subscription".into()) && !w(&|w| w.source = "manual".into()));
        assert!(w(&|w| w.min_size_mb = 50) && !w(&|w| w.min_size_mb = 51));
        assert!(w(&|w| {
            w.platform = "bilibili".into();
            w.title = "教程".into();
            w.min_size_mb = 10;
        }));
        assert!(!w(&|w| {
            w.platform = "bilibili".into();
            w.title = "教程".into();
            w.author = "别人".into();
        }));
    }

    #[test]
    fn disabled_rules_are_skipped_and_order_is_kept() {
        let rule = |id: &str, enabled: bool, title: &str| AutoRule {
            id: id.into(),
            name: id.into(),
            enabled,
            when: RuleWhen { title: title.into(), ..Default::default() },
            then: RuleThen::default(),
        };
        let rules = vec![rule("a", true, "教程"), rule("b", false, "教程"), rule("c", true, "不相关"), rule("d", true, "")];
        let hit: Vec<&str> = matching(&rules, &item()).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(hit, vec!["a", "d"]);
    }

    #[test]
    fn tags_favorite_and_move_are_applied() {
        let st = crate::db::Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("clearclip-rules-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.mp4");
        std::fs::write(&file, b"v").unwrap();
        st.record_download(&crate::db::NewDownload {
            platform: "bilibili",
            media_id: "BV1",
            asset_id: "video",
            title: "【教程】Rust 入门",
            author: "码农老张",
            cover: None,
            path: &file.to_string_lossy(),
            size: 1,
            kind: "video",
            source: "subscription",
            source_url: "",
            platform_name: "B站",
        })
        .unwrap();
        let id = st.id_by_key("bilibili", "BV1", "video").unwrap().unwrap();
        let it = st.library_item(id).unwrap().unwrap();
        let rule = AutoRule {
            id: "r".into(),
            name: "教程归档".into(),
            enabled: true,
            when: RuleWhen::default(),
            then: RuleThen { add_tags: vec!["学习".into(), "教程".into()], favorite: true, move_to: "学习/{author}".into(), ..Default::default() },
        };
        let (path, notes) = apply_local(&st, &dir, &it, &rule);
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(path.ends_with("学习/码农老张/a.mp4"), "{path}");
        assert!(Path::new(&path).is_file() && !file.exists(), "the file really moved");
        let after = st.library_item(id).unwrap().unwrap();
        assert_eq!(after.path, path, "the library follows the move");
        assert!(after.favorite);
        assert_eq!(after.tags.len(), 2);
        // 已经在目标位置：不再移动
        let (p2, n2) = apply_local(&st, &dir, &after, &rule);
        assert_eq!(p2, path);
        assert!(!n2.iter().any(|n| n.starts_with("移到")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
