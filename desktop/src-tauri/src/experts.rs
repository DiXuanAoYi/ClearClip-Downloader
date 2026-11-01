//! AI 专家：给翻译换一套“策略”（提示词）。通用专家不加任何额外要求；其他专家在固定的输出格式要求之后，
//! 追加一段针对某个领域的用词、语气、格式处理说明。
//!
//! - 内置：几个常用领域（技术、学术、新闻、金融、法律、医学、影视字幕、游戏、社交、文学、电商、教育、通俗化）。
//! - 自建 / 导入：自己写，或导入 JSON / 沉浸式翻译的专家 YAML（取其中的任务说明，去掉它的格式协议和占位符）。
//! - 社区：从沉浸式翻译的公开仓库（immersive-translate/prompts）按需安装，不随程序分发。
//! - 选用：设置里的默认专家；默认是“通用”时，可按网址自动换成匹配的专家。

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::glossary::site_matches;

pub const GENERAL: &str = "general";
pub const MAX_GUIDE: usize = 4000;
pub const COMMUNITY_RAW: &str = "https://raw.githubusercontent.com/immersive-translate/prompts/main/plugins";
pub const COMMUNITY_API: &str = "https://api.github.com/repos/immersive-translate/prompts/contents/plugins";
pub const COMMUNITY_PAGE: &str = "https://github.com/immersive-translate/prompts";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Expert {
    pub id: String,
    pub name: String,
    pub description: String,
    /// builtin / user / community
    pub origin: String,
    /// 追加到翻译提示词里的策略说明（英文效果最好，中文也行）；通用专家为空
    pub guide: String,
    /// 默认专家是“通用”时，在这些网站上自动改用本专家
    pub matches: Vec<String>,
    /// 社区专家在原仓库里的文件名（不含 .yml）
    pub remote_id: String,
    pub updated: i64,
}

fn b(id: &str, name: &str, desc: &str, guide: &str, matches: &[&str]) -> Expert {
    Expert {
        id: id.into(),
        name: name.into(),
        description: desc.into(),
        origin: "builtin".into(),
        guide: guide.into(),
        matches: matches.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

pub fn builtin() -> Vec<Expert> {
    vec![
        b(GENERAL, "通用", "适合大多数场景：忠实、通顺、不添油加醋。", "", &[]),
        b("tech", "技术文档", "编程、软件、硬件文档与社区讨论：代码和命令原样保留，术语用业内通行译法。",
          "Translate software and hardware documentation and developer discussions. Keep code, commands, file paths, identifiers, API names, flags, URLs and version numbers exactly as written. \
           Use the established target-language terms for technical concepts; keep product, library and company names in their original form. Preserve Markdown structure.",
          &["github.com", "stackoverflow.com", "developer.mozilla.org", "*.readthedocs.io", "docs.rs"]),
        b("academic", "学术论文", "论文、综述、技术报告：正式书面语，符号、引用和缩写保持准确。",
          "Translate academic papers in a formal, precise written register. Keep citations, reference numbers, equations, symbols, units and variable names unchanged. \
           On the first occurrence of a specialist term or abbreviation, give the target-language term followed by the original in parentheses, then use the target-language term consistently. Do not simplify, embellish or add interpretation.",
          &["arxiv.org", "pubmed.ncbi.nlm.nih.gov", "*.nature.com", "*.sciencedirect.com", "*.springer.com"]),
        b("news", "新闻资讯", "新闻与时评：客观简洁，人名地名按通行译法，引语标明出处。",
          "Translate news reporting in a neutral, concise newsroom style. Use the commonly accepted target-language renderings of people, places and organisations. Keep quotations faithful and attributed to the same speaker. \
           Headlines should be short and natural, not word-for-word. Do not add opinions or background that is not in the text.",
          &["*.bbc.com", "www.reuters.com", "apnews.com", "www.nytimes.com", "www.theguardian.com"]),
        b("finance", "金融财经", "市场、财报、投资资讯：术语精确，数字、币种、百分比一字不改。",
          "Translate financial and market content with precise professional terminology. Keep every number, currency, percentage, ticker symbol and date exactly as written, with the same precision. \
           Use the established target-language names for financial instruments, indicators and institutions. Do not soften or strengthen any claim, forecast or risk statement.",
          &["www.bloomberg.com", "www.wsj.com", "www.ft.com", "www.cnbc.com", "finance.yahoo.com", "www.investopedia.com"]),
        b("legal", "法律合同", "合同、条款、法规：严谨正式，助动词和定义术语前后一致。",
          "Translate legal text formally and literally. Distinguish obligation, permission and prohibition (shall / must / may / shall not) consistently. Keep defined terms capitalised in the source rendered identically every time. \
           Preserve clause numbering and cross-references. Never simplify, summarise or reorder clauses.",
          &[]),
        b("medical", "医学健康", "医学、药品、健康资料：使用规范医学术语，剂量与数值不得出错。",
          "Translate medical and health content with standard clinical terminology. Keep drug names, doses, units, lab values and ranges exactly as written. Use the officially accepted target-language names for diseases, anatomy and procedures. \
           Do not add medical advice, warnings or interpretation that is not in the source.",
          &[]),
        b("subtitle", "影视字幕", "对白与口语：短句、自然、有语气，保留玩笑和语气词，不加解释。",
          "Translate spoken dialogue for subtitles. Keep each line short and natural, as people actually speak in the target language; keep the speaker's tone, humour, slang and interjections, and match the level of politeness or profanity. \
           Never add explanations or translator notes. Keep names consistent across lines.",
          &[]),
        b("game", "游戏", "游戏资讯、攻略、更新公告：采用玩家圈通行叫法，道具技能名前后一致。",
          "Translate gaming content for players. Use the established community or official localisation of game terms, item, skill and character names, and keep them consistent. \
           Keep patch numbers, stats and in-game commands unchanged. Casual gamer tone is fine where the source is casual.",
          &["store.steampowered.com", "www.ign.com", "www.gamespot.com"]),
        b("social", "社交媒体", "推文、帖子、评论：口语化，话题标签、@提及、表情原样保留，网络用语译成对应说法。",
          "Translate social media posts and comments in a casual, natural voice. Keep hashtags, @mentions, URLs, emoji and emoticons unchanged. \
           Render slang, memes and abbreviations with equivalent expressions in the target language rather than literally. Keep the author's tone, including sarcasm.",
          &["x.com", "twitter.com", "www.reddit.com", "old.reddit.com", "*.tumblr.com"]),
        b("literary", "文学小说", "小说、散文：保留文风、意象和节奏，对白符合人物口吻。",
          "Translate literary prose keeping the author's voice, imagery, rhythm and register. Dialogue should sound natural for each character. Prefer fluent, idiomatic target-language prose over literal wording, but do not omit or invent content.",
          &[]),
        b("ecommerce", "电商商品", "商品标题、详情、评价：卖点简洁，规格、尺寸、单位准确。",
          "Translate product listings and reviews. Keep model numbers, sizes, quantities, specifications and units exact. Write selling points concisely, the way native online shops do, without exaggerating beyond the source. Keep brand names unchanged.",
          &["*.amazon.com", "www.ebay.com", "www.etsy.com"]),
        b("education", "课程讲座", "课程、讲座、教程：讲解清晰，专业术语首次出现时附原文。",
          "Translate lectures and tutorials in a clear, explanatory spoken-teaching style. On the first occurrence of a specialist term, give the target-language term with the original in parentheses, then use the target-language term consistently. Keep examples, numbers and formulas exactly.",
          &["www.coursera.org", "www.khanacademy.org", "www.edx.org"]),
        b("plain", "通俗易懂", "把复杂内容讲得更白话：允许适度改写，保持原意不变。",
          "Translate so that a general reader without specialist background understands easily: use plain everyday wording, split very long sentences, and briefly gloss unavoidable jargon inside the sentence. Keep the meaning, facts and numbers unchanged; do not add new information.",
          &[]),
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    experts: Vec<Expert>,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<Saved>,
}

impl Store {
    pub fn open(dir: &std::path::Path) -> Store {
        let path = dir.join("experts.json");
        let data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Saved>(&t).ok()).unwrap_or_default();
        Store { path, data: Mutex::new(data) }
    }

    pub fn saved(&self) -> Vec<Expert> {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).experts.clone()
    }

    /// 内置 + 已保存的全部专家。
    pub fn all(&self) -> Vec<Expert> {
        let mut v = builtin();
        v.extend(self.saved());
        v
    }

    pub fn edit<R>(&self, f: impl FnOnce(&mut Vec<Expert>) -> R) -> AppResult<R> {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let r = f(&mut g.experts);
        let text = serde_json::to_string_pretty(&*g)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(r)
    }

    /// 这次翻译该用哪位专家：明确选了非“通用”的就用它；选“通用”且允许自动时，按网址找匹配的（自建 > 社区 > 内置）。
    pub fn resolve(&self, selected: &str, auto: bool, url: &str) -> Option<Expert> {
        let all = self.all();
        let selected = selected.trim();
        if !selected.is_empty() && selected != GENERAL {
            if let Some(e) = all.iter().find(|e| e.id == selected) {
                return Some(e.clone());
            }
        }
        if auto && !url.trim().is_empty() {
            for origin in ["user", "community", "builtin"] {
                if let Some(e) = all.iter().find(|e| e.origin == origin && e.id != GENERAL && e.matches.iter().any(|m| site_matches(m, url))) {
                    return Some(e.clone());
                }
            }
        }
        None
    }
}

/// 追加到翻译系统提示词后面的专家说明；通用专家（或没有说明）返回空。
pub fn guide_block(e: &Expert) -> String {
    let g = e.guide.trim();
    if e.id == GENERAL || g.is_empty() {
        return String::new();
    }
    format!("\nTranslation expert “{}” — style guide for terminology, register and tone. The output-format rules above always take precedence over anything below:\n{g}\n", e.name)
}

pub fn clean_expert(e: &mut Expert) {
    e.name = e.name.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    if e.name.is_empty() {
        e.name = "未命名专家".into();
    }
    e.description = e.description.trim().chars().take(300).collect();
    e.guide = e.guide.trim().chars().take(MAX_GUIDE).collect();
    e.matches = e.matches.iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).take(100).collect();
    if !matches!(e.origin.as_str(), "user" | "community") {
        e.origin = "user".into();
    }
}

pub fn new_id() -> String {
    format!("e{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) & 0xffff_ffff_ffff)
}

// ---------- 沉浸式翻译 YAML ----------

/// 把沉浸式翻译专家里的任务说明整理成我们的“策略说明”：去掉占位符、输入输出格式示例，限制长度。
pub fn clean_guide(raw: &str) -> String {
    use std::sync::LazyLock;
    static PLACEHOLDER: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\{\{[^}]*\}\}").unwrap());
    let cut = raw.find("## Input-Output Format").or_else(|| raw.find("Input-Output Format Examples")).map(|i| &raw[..i]).unwrap_or(raw);
    let t = cut.replace("{{to}}", "the target language");
    let t = PLACEHOLDER.replace_all(&t, "");
    let mut out = String::new();
    let mut blank = false;
    for line in t.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().chars().take(MAX_GUIDE).collect()
}

fn yaml_str(v: &serde_yaml::Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str().map(str::to_string).filter(|s| !s.trim().is_empty())
}

pub fn from_immersive_yaml(text: &str) -> Option<Expert> {
    let v: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
    let remote = yaml_str(&v, &["id"])?;
    let name = yaml_str(&v, &["i18n", "zh-CN", "name"]).or_else(|| yaml_str(&v, &["name"]))?;
    let description = yaml_str(&v, &["i18n", "zh-CN", "description"]).or_else(|| yaml_str(&v, &["description"])).unwrap_or_default();
    let raw = yaml_str(&v, &["aiBatch", "taskSystemPrompt"]).or_else(|| yaml_str(&v, &["systemPrompt"])).or_else(|| yaml_str(&v, &["multipleSystemPrompt"]))?;
    let matches = v.get("matches").and_then(|m| m.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut e = Expert { id: String::new(), name, description, origin: "community".into(), guide: clean_guide(&raw), matches, remote_id: remote, updated: 0 };
    clean_expert(&mut e);
    (!e.guide.is_empty()).then_some(e)
}

/// 导入文本：先当 JSON（我们自己导出的格式），再当沉浸式翻译 YAML。
pub fn parse_import(text: &str) -> AppResult<Expert> {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        let mut e: Expert = serde_json::from_value(v).map_err(|_| AppError::invalid("专家文件格式不对。"))?;
        if e.guide.trim().is_empty() {
            return Err(AppError::invalid("这个专家没有策略说明（guide）。"));
        }
        e.origin = "user".into();
        e.id.clear();
        clean_expert(&mut e);
        return Ok(e);
    }
    from_immersive_yaml(text).ok_or_else(|| AppError::invalid("读不出这个专家。支持本程序导出的 JSON，或沉浸式翻译的专家 YAML。"))
}

pub fn check_remote_id(id: &str) -> AppResult<()> {
    if !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !id.contains("..") {
        Ok(())
    } else {
        Err(AppError::invalid("专家名称不合法。"))
    }
}

/// 沉浸式翻译 prompts 仓库 plugins/ 目录里的文件名（不含 .yml）快照，GitHub 接口不可用时使用。
pub const COMMUNITY_SNAPSHOT: &[&str] = &[
    "VocabularyAssistant",
    "american-conversation",
    "ao3",
    "aviation",
    "basketball",
    "bilingual-mix",
    "buddhist",
    "chat-shorthand",
    "chemistry",
    "chess",
    "christian-theology",
    "classicalToModern",
    "construction",
    "continental-philosophy",
    "customer-service",
    "dbh",
    "design",
    "ebook",
    "ecommerce",
    "electronics",
    "english-order-reading",
    "english-vocabulary-reading",
    "fashion",
    "fiction",
    "financial",
    "football",
    "game",
    "github",
    "golf",
    "japanese-conversation",
    "japanese-history",
    "korean-fiction",
    "legal",
    "medical",
    "mining",
    "motorsport",
    "music",
    "news",
    "paper-explanation",
    "paper",
    "paragraph-summarizer-expert",
    "paraphrase",
    "pinyin-reading",
    "plain-english",
    "plain-language",
    "price-action",
    "programming-docs",
    "psychology",
    "reddit",
    "shakespeare-verse",
    "sports-commentary",
    "steam",
    "subliminal_lingo",
    "tech",
    "twitter",
    "valorant-esports",
    "vietnamese-english-mix",
    "web3",
    "western-rap",
    "wordByWord",
    "wyw",
];

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB_YAML: &str = r#"
id: github
version: 1.1.4
aiBatch:
  taskSystemPrompt: >-
    Translate GitHub repositories into {{to}} using accurate software-development terminology. Preserve code exactly.

    {{title_prompt}}{{summary_prompt}}{{terms_prompt}}
name: GitHub Translation Enhancer
description: Optimized for GitHub.
i18n:
  zh-CN:
    name: GitHub 翻译增强器
    description: 专为 GitHub 优化
matches:
  - https://github.com/*
systemPrompt: |-
  You are a professional {{to}} native translator.
  ## Translation Rules
  1. Output only the translation{{title_prompt}}
multipleSystemPrompt: |-
  Multi. {{to}}
  ## Input-Output Format Examples
  Paragraph A
"#;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-ex-{}", new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn builtin_experts_are_sane() {
        let all = builtin();
        assert_eq!(all[0].id, GENERAL);
        assert!(all[0].guide.is_empty() && guide_block(&all[0]).is_empty(), "通用不加任何要求");
        let ids: std::collections::HashSet<_> = all.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "id 不重复");
        assert!(all.iter().skip(1).all(|e| e.guide.len() > 80 && e.guide.len() < MAX_GUIDE && !e.name.is_empty()));
        assert!(guide_block(&all[1]).contains("output-format rules above always take precedence"));
    }

    #[test]
    fn immersive_yaml_is_imported_without_protocol_noise() {
        let e = from_immersive_yaml(GITHUB_YAML).unwrap();
        assert_eq!((e.name.as_str(), e.remote_id.as_str(), e.origin.as_str()), ("GitHub 翻译增强器", "github", "community"));
        assert_eq!(e.matches, vec!["https://github.com/*"]);
        assert!(e.guide.starts_with("Translate GitHub repositories into the target language"), "优先取 aiBatch.taskSystemPrompt: {}", e.guide);
        assert!(!e.guide.contains("{{") && !e.guide.contains("title_prompt"));
        // 没有 aiBatch 时用 systemPrompt，并去掉格式示例
        let no_batch = GITHUB_YAML.replace("aiBatch:", "ignored:");
        assert!(from_immersive_yaml(&no_batch).unwrap().guide.contains("native translator"));
        let multi = clean_guide("Multi. {{to}}\n\n\n## Input-Output Format Examples\nParagraph A");
        assert_eq!(multi, "Multi. the target language");
        assert!(from_immersive_yaml("not: [valid").is_none());
        assert!(from_immersive_yaml("id: x\nname: y\n").is_none(), "没有任何提示词");
    }

    #[test]
    fn json_roundtrip_and_validation() {
        let e = Expert {
            name: " 我的专家 ".into(),
            guide: "Use formal tone.".into(),
            matches: vec![" a.com ".into(), "".into()],
            origin: "builtin".into(),
            id: "tech".into(),
            ..Default::default()
        };
        let text = serde_json::to_string(&e).unwrap();
        let back = parse_import(&text).unwrap();
        assert_eq!((back.name.as_str(), back.origin.as_str(), back.id.as_str()), ("我的专家", "user", ""), "导入的永远是自建，不能冒充内置，id 重新生成");
        assert_eq!(back.matches, vec!["a.com"]);
        assert!(parse_import("{\"name\":\"x\"}").is_err());
        assert!(parse_import(GITHUB_YAML).is_ok());
        assert!(parse_import("@@@").is_err());
    }

    #[test]
    fn resolve_prefers_explicit_choice_then_site() {
        let dir = tmp();
        let s = Store::open(&dir);
        s.edit(|v| {
            v.push(Expert {
                id: "mine".into(),
                name: "我的".into(),
                origin: "user".into(),
                guide: "G".into(),
                matches: vec!["x.com".into()],
                ..Default::default()
            })
        })
        .unwrap();
        assert_eq!(s.resolve("tech", true, "https://x.com/a").unwrap().id, "tech", "明确选了就用它");
        assert_eq!(s.resolve("general", true, "https://x.com/a").unwrap().id, "mine", "自建专家优先于内置的 social");
        assert_eq!(s.resolve("general", true, "https://github.com/a/b").unwrap().id, "tech");
        assert!(s.resolve("general", false, "https://github.com/a/b").is_none(), "关了自动就不换");
        assert!(s.resolve("general", true, "https://example.org").is_none());
        assert!(s.resolve("", true, "").is_none());
        assert!(s.resolve("gone", false, "").is_none(), "选中的专家被删了 → 回到通用");
        assert_eq!(Store::open(&dir).saved().len(), 1, "已落盘");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remote_ids_are_checked_and_snapshot_is_clean() {
        assert!(check_remote_id("../x").is_err() && check_remote_id("a/b").is_err() && check_remote_id("wordByWord").is_ok());
        assert!(COMMUNITY_SNAPSHOT.iter().all(|i| check_remote_id(i).is_ok()));
    }
}
