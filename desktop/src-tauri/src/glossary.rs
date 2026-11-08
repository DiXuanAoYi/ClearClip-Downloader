//! AI 术语库：翻译时让模型按固定译法翻译专有名词。
//!
//! - 术语库 = 一组「原文 → 译文」，可限定生效的网站和目标语言；来源有自建、社区（沉浸式翻译的 terms 仓库）、智能提取。
//! - 智能提取：让大模型从文本 / 字幕 / 网页里挑出专有名词并给出译法，先进「待确认」，由你确认后加入某个术语库。
//! - 提取网站：这些网站上翻译网页 / 视频时，自动在后台提取术语，进「待确认」。
//! - 翻译时：只把本批文字里**出现过**的术语放进提示词，省 token，也不干扰无关内容。
//!
//! - 术语可以分层（如 界面 › 文件 › 打开文件）：上级本身也是术语；翻译时命中的术语连同上级路径一起交给模型，
//!   同一个原文在不同上级下可以有不同译法。上级可以设成“只作分组”（不当术语用）。
//!
//! CSV 与沉浸式翻译一致：`source,target,tgt_lng`（本程序另外认 `note`、`path`（上级路径，用 / 分隔）、`group` 三列，对方会忽略）。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 单个术语库最多多少条；整体提示词里一次最多放多少条。
pub const MAX_TERMS: usize = 20_000;
pub const MAX_IN_PROMPT: usize = 40;
/// 术语最多几层
pub const MAX_DEPTH: usize = 6;
const MAX_CANDIDATES: usize = 2_000;

pub const COMMUNITY_RAW: &str = "https://raw.githubusercontent.com/immersive-translate/terms/main";
pub const COMMUNITY_PAGE: &str = "https://github.com/immersive-translate/terms";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Term {
    pub source: String,
    /// 译文；留空表示“保持原文不翻译”
    pub target: String,
    pub note: String,
    /// 下级术语（分层）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Term>,
    /// 只作分组：不当术语用，下级照常用
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub group: bool,
    /// 翻译时用：上级路径和上级的说明（`flatten` 填写，不存盘）
    #[serde(skip)]
    pub context: String,
}

impl Term {
    pub fn new(source: &str, target: &str) -> Term {
        Term { source: source.into(), target: target.into(), ..Default::default() }
    }
}

/// 一组术语（含所有下级）一共多少条。
pub fn count_terms(terms: &[Term]) -> usize {
    terms.iter().map(|t| 1 + count_terms(&t.children)).sum()
}

/// 摊平成一层，给翻译用：只作分组的不要；有上级的在 `context` 里写上路径（原文 › … = 译文 › …）和上级的说明。
pub fn flatten(terms: &[Term]) -> Vec<Term> {
    fn walk(ts: &[Term], src: &mut Vec<String>, tgt: &mut Vec<String>, notes: &mut Vec<String>, out: &mut Vec<Term>) {
        for t in ts {
            if !t.group {
                let mut ctx = vec![];
                if !src.is_empty() {
                    ctx.push(format!("in {} = {}", src.join(" › "), tgt.join(" › ")));
                }
                ctx.extend(notes.iter().filter(|n| !n.is_empty()).cloned());
                out.push(Term {
                    source: t.source.clone(),
                    target: t.target.clone(),
                    note: t.note.clone(),
                    children: vec![],
                    group: false,
                    context: ctx.join("; "),
                });
            }
            if !t.children.is_empty() {
                src.push(t.source.clone());
                tgt.push(if t.target.is_empty() { t.source.clone() } else { t.target.clone() });
                notes.push(t.note.clone());
                walk(&t.children, src, tgt, notes, out);
                src.pop();
                tgt.pop();
                notes.pop();
            }
        }
    }
    let mut out = vec![];
    walk(terms, &mut vec![], &mut vec![], &mut vec![], &mut out);
    out
}

/// 把 `incoming` 并进 `into`：同一层里同名（不分大小写）的以新的为准，下级逐层合并；新的追加在后面。
pub fn merge_terms(into: &mut Vec<Term>, incoming: Vec<Term>) {
    for t in incoming {
        let key = t.source.trim().to_lowercase();
        if let Some(have) = into.iter_mut().find(|x| x.source.trim().to_lowercase() == key) {
            have.target = t.target;
            have.note = t.note;
            have.group = t.group;
            merge_terms(&mut have.children, t.children);
        } else {
            into.push(t);
        }
    }
}

/// 按上级路径（原文）放进树里：路径上缺的上级补成“只作分组”。
fn insert_at(tree: &mut Vec<Term>, path: &[String], t: Term) {
    let Some((head, rest)) = path.split_first() else {
        // CSV 里同一层重复的原文保留靠前的；之前为了放下级补出来的分组，用这一行的内容填上
        let key = t.source.trim().to_lowercase();
        match tree.iter_mut().find(|x| x.source.trim().to_lowercase() == key) {
            Some(have) if have.group && have.target.is_empty() && have.note.is_empty() => {
                have.target = t.target;
                have.note = t.note;
                have.group = t.group;
            }
            Some(_) => {}
            None => tree.push(t),
        }
        return;
    };
    let key = head.to_lowercase();
    let idx = match tree.iter().position(|x| x.source.trim().to_lowercase() == key) {
        Some(i) => i,
        None => {
            tree.push(Term { source: head.clone(), group: true, ..Default::default() });
            tree.len() - 1
        }
    };
    insert_at(&mut tree[idx].children, rest, t);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Glossary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// user / community / extracted
    pub origin: String,
    /// 社区术语库的 meta 名称，用来检查更新
    pub remote_id: String,
    /// 生效网站：`example.com`、`*.example.com`、`https://x.com/path*`、`*`；空 = 所有网站
    pub matches: Vec<String>,
    /// 适用的目标语言（zh-CN / zh-TW / en …）；空或 auto = 任意
    pub lang: String,
    pub terms: Vec<Term>,
    pub updated: i64,
    /// 所在文件夹，多层用 `/` 分隔（如 `游戏/原神`）；空 = 最外层
    pub folder: String,
}

impl Default for Glossary {
    fn default() -> Self {
        Glossary {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            enabled: true,
            origin: "user".into(),
            remote_id: String::new(),
            matches: vec![],
            lang: String::new(),
            terms: vec![],
            updated: 0,
            folder: String::new(),
        }
    }
}

/// 智能提取得到、等待确认的术语。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Candidate {
    /// `站点|小写原文`
    pub id: String,
    pub source: String,
    pub target: String,
    pub note: String,
    /// 来自哪个网站（手动提取为空）
    pub site: String,
    pub count: u32,
    pub found: i64,
}

/// 需要自动提取术语的网站。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtractSite {
    pub pattern: String,
    pub enabled: bool,
    /// 提取到的术语直接加入这个术语库；空 = 只进待确认
    pub glossary: String,
    pub last: i64,
    pub found: u32,
}

impl Default for ExtractSite {
    fn default() -> Self {
        ExtractSite { pattern: String::new(), enabled: true, glossary: String::new(), last: 0, found: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Data {
    pub glossaries: Vec<Glossary>,
    pub candidates: Vec<Candidate>,
    pub sites: Vec<ExtractSite>,
    /// 建了还没放术语库的文件夹（有术语库的文件夹由术语库自己的 `folder` 体现）
    pub folders: Vec<String>,
}

// ---------- 文件夹 ----------

/// 整理文件夹路径：`/` 分隔，去掉空段和首尾空白，最多 6 层、每层 40 字。
pub fn clean_folder(path: &str) -> String {
    path.split(['/', '\\'])
        .map(|p| p.trim().chars().filter(|c| !c.is_control()).take(40).collect::<String>())
        .filter(|p| !p.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("/")
}

/// `path` 是不是 `dir` 本身或它下面的。
pub fn in_folder(path: &str, dir: &str) -> bool {
    path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// 把 `from` 文件夹（连同子文件夹和里面的术语库）改到 `to` 下。
pub fn rename_folder(d: &mut Data, from: &str, to: &str) {
    let mv = |p: &str| if in_folder(p, from) { clean_folder(&format!("{to}{}", &p[from.len()..])) } else { p.to_string() };
    for g in &mut d.glossaries {
        g.folder = mv(&g.folder);
    }
    d.folders = d.folders.iter().map(|f| mv(f)).filter(|f| !f.is_empty()).collect();
    d.folders.sort();
    d.folders.dedup();
}

/// 删除文件夹：里面的术语库和子文件夹往上挪一层（不删术语库）。
pub fn remove_folder(d: &mut Data, path: &str) {
    let parent = path.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
    let up = |p: &str| -> String {
        if p == path {
            parent.clone()
        } else if in_folder(p, path) {
            clean_folder(&format!("{parent}/{}", &p[path.len() + 1..]))
        } else {
            p.to_string()
        }
    };
    for g in &mut d.glossaries {
        g.folder = up(&g.folder);
    }
    d.folders = d.folders.iter().filter(|f| f.as_str() != path).map(|f| up(f)).filter(|f| !f.is_empty()).collect();
    d.folders.sort();
    d.folders.dedup();
}

// ---------- 持久化 ----------

pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Store {
    pub fn open(dir: &std::path::Path) -> Store {
        let path = dir.join("glossaries.json");
        let data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Data>(&t).ok()).unwrap_or_default();
        Store { path, data: Mutex::new(data) }
    }

    pub fn snapshot(&self) -> Data {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 修改并写盘。
    pub fn edit<R>(&self, f: impl FnOnce(&mut Data) -> R) -> AppResult<R> {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let r = f(&mut g);
        let text = serde_json::to_string_pretty(&*g)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(r)
    }

    /// 翻译 `url` 所在网站 / 视频到 `lang_tag`（`ai::lang_tag` 的结果）时可用的术语，去重后按术语库顺序。
    pub fn terms_for(&self, url: &str, lang_tag: &str) -> Vec<Term> {
        select_terms(&self.snapshot().glossaries, url, lang_tag)
    }

    pub fn site_for(&self, url: &str) -> Option<ExtractSite> {
        self.snapshot().sites.into_iter().find(|s| s.enabled && site_matches(&s.pattern, url))
    }
}

pub fn new_id() -> String {
    format!("g{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) & 0xffff_ffff_ffff)
}

/// 整理用户提交的术语库：去空行、去重（保留靠前的）、限制数量。
pub fn clean_glossary(g: &mut Glossary) {
    g.name = g.name.trim().chars().filter(|c| !c.is_control()).take(60).collect();
    if g.name.is_empty() {
        g.name = "未命名术语库".into();
    }
    g.description = g.description.trim().chars().take(400).collect();
    g.matches = g.matches.iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).take(100).collect();
    g.lang = g.lang.trim().to_string();
    g.folder = clean_folder(&g.folder);
    if !matches!(g.origin.as_str(), "user" | "community" | "extracted") {
        g.origin = "user".into();
    }
    g.terms = clean_terms(std::mem::take(&mut g.terms));
}

/// 整理术语：去掉空的、同一层里重名的合并（保留靠前的，下级合在一起）、限制总条数和层数（超过 6 层的提到第 6 层）。
pub fn clean_terms(terms: Vec<Term>) -> Vec<Term> {
    let mut budget = MAX_TERMS;
    clean_level(terms, 1, &mut budget)
}

fn clean_level(terms: Vec<Term>, depth: usize, budget: &mut usize) -> Vec<Term> {
    // 先摊开：原文为空的行不要，但它的下级提上来
    let mut flat: Vec<Term> = vec![];
    let mut stack: Vec<Term> = terms.into_iter().rev().collect();
    while let Some(mut t) = stack.pop() {
        t.source = t.source.trim().chars().filter(|c| !c.is_control()).take(200).collect();
        if t.source.is_empty() {
            stack.extend(std::mem::take(&mut t.children).into_iter().rev());
            continue;
        }
        // 已经是最深一层：下级提到这一层，排在它后面
        if depth >= MAX_DEPTH && !t.children.is_empty() {
            let kids = std::mem::take(&mut t.children);
            flat.push(t);
            stack.extend(kids.into_iter().rev());
            continue;
        }
        flat.push(t);
    }
    // 同一层重名的合并
    let mut order: Vec<Term> = vec![];
    let mut at: HashMap<String, usize> = HashMap::new();
    for t in flat {
        let key = t.source.to_lowercase();
        if let Some(&i) = at.get(&key) {
            order[i].children.extend(t.children);
        } else {
            at.insert(key, order.len());
            order.push(t);
        }
    }
    let mut out = vec![];
    for t in order {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let children = clean_level(t.children, depth + 1, budget);
        out.push(Term {
            source: t.source,
            target: t.target.trim().chars().take(300).collect(),
            note: t.note.trim().chars().take(200).collect(),
            children,
            group: t.group,
            context: String::new(),
        });
    }
    out
}

// ---------- CSV ----------

/// 解析一行 CSV（支持双引号、`""` 转义）。
fn csv_row(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// 解析术语 CSV：表头 `source,target[,tgt_lng][,note]`（没有表头就按前两列）。也认制表符分隔。
pub fn parse_csv(text: &str) -> Vec<Term> {
    let text = text.trim_start_matches('\u{feff}');
    let mut terms = vec![];
    // 原文、译文、说明、上级路径、只作分组 各在第几列
    type Cols = (usize, usize, Option<usize>, Option<usize>, Option<usize>);
    let mut cols: Option<Cols> = None;
    let mut first = true;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let row = if line.contains('\t') && !line.contains(',') { line.split('\t').map(str::to_string).collect() } else { csv_row(line) };
        if first {
            first = false;
            let low: Vec<String> = row.iter().map(|c| c.trim().to_lowercase()).collect();
            if let Some(si) = low.iter().position(|c| c == "source") {
                if let Some(ti) = low.iter().position(|c| c == "target") {
                    let col = |n: &[&str]| low.iter().position(|c| n.contains(&c.as_str()));
                    cols = Some((si, ti, col(&["note"]), col(&["path", "parent"]), col(&["group"])));
                    continue;
                }
            }
        }
        let (si, ti, ni, pi, gi) = cols.unwrap_or((0, 1, None, None, None));
        let get = |i: usize| row.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let source = get(si);
        if source.is_empty() {
            continue;
        }
        let group = gi.map(get).is_some_and(|g| matches!(g.to_lowercase().as_str(), "1" | "true" | "yes" | "是"));
        let path: Vec<String> = pi.map(get).unwrap_or_default().split('/').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        insert_at(&mut terms, &path, Term { source, target: get(ti), note: ni.map(get).unwrap_or_default(), group, ..Default::default() });
    }
    clean_terms(terms)
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 导出成沉浸式翻译同样的 CSV（多 note、path（上级路径）、group 三列，放在最后，对方会忽略）。上级排在下级前面。
pub fn to_csv(terms: &[Term], lang: &str) -> String {
    fn walk(ts: &[Term], path: &mut Vec<String>, lang: &str, out: &mut String) {
        for t in ts {
            let g = if t.group { "1" } else { "" };
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv_cell(&t.source),
                csv_cell(&t.target),
                csv_cell(lang),
                csv_cell(&t.note),
                csv_cell(&path.join("/")),
                g
            ));
            path.push(t.source.clone());
            walk(&t.children, path, lang, out);
            path.pop();
        }
    }
    let lang = if lang == "auto" { "" } else { lang };
    let mut out = String::from("source,target,tgt_lng,note,path,group\n");
    walk(terms, &mut vec![], lang, &mut out);
    out
}

// ---------- 网站与语言匹配 ----------

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .or_else(|| {
            let t = url.trim().to_lowercase();
            (!t.is_empty() && !t.contains('/') && !t.contains(' ')).then_some(t)
        })
        .unwrap_or_default()
}

fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti] || p[pi] == '?') {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 屏幕翻译等“不是网页”的场合，用软件名当作网址：`app://photoshop`。
/// 术语库 / AI 专家的生效范围里写 `app:photoshop`（或直接写 `photoshop.exe`）就只在这个软件里生效。
pub fn app_url(exe: &str) -> String {
    let k = app_key(exe);
    if k.is_empty() {
        String::new()
    } else {
        format!("app://{k}")
    }
}

/// 软件名的统一写法：小写，去掉 `app://` / `app:` 前缀和 `.exe`，空格和斜杠换成 `-`。
pub fn app_key(s: &str) -> String {
    let t = s.trim().to_lowercase();
    let t = t.strip_prefix("app://").or_else(|| t.strip_prefix("app:")).unwrap_or(&t);
    let t = t.rsplit(['\\', '/']).next().unwrap_or(t);
    t.trim().trim_end_matches(".exe").trim().replace(' ', "-")
}

/// 与沉浸式翻译 `matches` 相同的写法：精确域名、`*.域名`（只匹配子域名）、完整网址（可含 `*`）、`*`。
pub fn site_matches(pattern: &str, url: &str) -> bool {
    let pat = pattern.trim().to_lowercase();
    if pat.is_empty() {
        return false;
    }
    if pat == "*" || pat == "*://*/*" {
        return true;
    }
    // 软件：网址是 app://名字 时，只看名字；写 app:名字 / app://名字 / 名字.exe 都行
    if url.trim().to_lowercase().starts_with("app://") {
        let want = app_key(url);
        let is_app_pat = pat.starts_with("app:") || pat.ends_with(".exe");
        return is_app_pat && !want.is_empty() && app_key(&pat) == want;
    }
    if pat.starts_with("app:") {
        return false;
    }
    let host = host_of(url);
    if pat.contains("://") {
        return glob(&pat, &url.trim().to_lowercase());
    }
    if host.is_empty() {
        return false;
    }
    if let Some(rest) = pat.strip_prefix("*.") {
        return host.ends_with(&format!(".{rest}"));
    }
    host == pat
}

/// 术语库的语言是否适用于目标语言标记（`ai::lang_tag`：zh / zh-Hant / en …）。
pub fn lang_ok(glossary_lang: &str, tag: &str) -> bool {
    let g = glossary_lang.trim().to_lowercase();
    if g.is_empty() || g == "auto" {
        return true;
    }
    let trad = matches!(g.as_str(), "zh-tw" | "zh-hk" | "zh-hant" | "zh-mo");
    match tag {
        "zh-Hant" => trad,
        "zh" => g == "zh" || g == "zh-cn" || g == "zh-sg" || g == "zh-hans",
        "ai" => true, // 认不出目标语言时不过滤
        other => g == other || g.split('-').next() == Some(other),
    }
}

pub fn select_terms(glossaries: &[Glossary], url: &str, tag: &str) -> Vec<Term> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    // 自建 > 提取 > 社区；同一来源里靠前的优先
    for origin in ["user", "extracted", "community"] {
        for g in glossaries.iter().filter(|g| g.enabled && g.origin == origin) {
            let site_ok = g.matches.is_empty() || (!url.trim().is_empty() && g.matches.iter().any(|m| site_matches(m, url)));
            if !site_ok || !lang_ok(&g.lang, tag) {
                continue;
            }
            // 同一个原文在不同上级下可以有不同译法，所以按“路径 + 原文”去重
            for t in flatten(&g.terms) {
                if seen.insert(format!("{}|{}", t.context.to_lowercase(), t.source.to_lowercase())) {
                    out.push(t);
                }
            }
        }
    }
    out
}

// ---------- 放进提示词 ----------

fn occurs(haystack_lower: &str, needle: &str) -> bool {
    let n = needle.to_lowercase();
    if n.is_empty() {
        return false;
    }
    // 英文单词要整词匹配（避免 “art” 命中 “party”）；含 CJK 或符号的直接按子串
    let plain = n.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '\'');
    if !plain {
        return haystack_lower.contains(&n);
    }
    let mut from = 0;
    while let Some(pos) = haystack_lower[from..].find(&n) {
        let a = from + pos;
        let b = a + n.len();
        let before = haystack_lower[..a].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
        let after = haystack_lower[b..].chars().next().map_or(true, |c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = b;
    }
    false
}

/// 这批文字里出现过的术语，长的优先，最多 `max` 条。
pub fn relevant<'a>(terms: &'a [Term], texts: &[String], max: usize) -> Vec<&'a Term> {
    if terms.is_empty() {
        return vec![];
    }
    let hay = texts.join("\n").to_lowercase();
    let mut hit: Vec<&Term> = terms.iter().filter(|t| occurs(&hay, &t.source)).collect();
    hit.sort_by_key(|t| std::cmp::Reverse(t.source.chars().count()));
    hit.truncate(max);
    hit
}

/// 接在翻译系统提示词后面的一段；没有命中的术语时返回空。
pub fn prompt_block(terms: &[&Term]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    let mut s = String::from("\nGlossary — whenever these terms appear, use exactly the given rendering (match case-insensitively; inflections follow the same rendering). A term with no rendering must be left in the original language. Some terms say where they belong (\"in A › B = …\", e.g. a menu path or category); when the same term is listed with different renderings, use the one whose place fits the text:\n");
    for t in terms {
        let extra: Vec<&str> = [t.context.as_str(), t.note.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
        let note = if extra.is_empty() { String::new() } else { format!("  ({})", extra.join("; ")) };
        if t.target.is_empty() {
            s.push_str(&format!("- {} => (keep as is){note}\n", t.source));
        } else {
            s.push_str(&format!("- {} => {}{note}\n", t.source, t.target));
        }
    }
    s
}

// ---------- 智能提取 ----------

pub fn extract_system(lang: &str) -> String {
    format!(
        "You build a translation glossary. From the user's text pick the terms that a translator must render consistently into {lang}: proper nouns (people, products, organisations, places, characters), \
         domain-specific technical terms, abbreviations and recurring jargon or slang. Skip common everyday words and anything that is obviously translated the same way by every translator. \
         Reply with ONE JSON array only, no code fences: [{{\"source\":\"term as written\",\"target\":\"best rendering in {lang}; an empty string if it should stay untranslated\",\"note\":\"at most 8 words of context, may be empty\"}}]. \
         At most 40 items, most important first, no duplicates, source at most 6 words."
    )
}

/// 解析模型回答：数组，或 `{terms:[…]}`；字段名容错。
pub fn parse_extracted(resp: &str) -> Vec<Term> {
    let t = crate::ai::strip_fences(resp);
    let arr: Vec<Value> = if let (Some(a), Some(b)) = (t.find('['), t.rfind(']')) { serde_json::from_str(&t[a..=b]).unwrap_or_default() } else { vec![] };
    let get = |v: &Value, keys: &[&str]| keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str)).unwrap_or("").trim().to_string();
    let terms = arr
        .iter()
        .map(|v| Term {
            source: get(v, &["source", "term", "original"]),
            target: get(v, &["target", "translation"]),
            note: get(v, &["note", "context"]),
            ..Default::default()
        })
        .filter(|t| t.source.chars().count() <= 60 && !t.source.is_empty() && !t.source.eq_ignore_ascii_case(&t.target))
        .collect();
    clean_terms(terms)
}

/// 长文本取样成几段（均匀抽取），每段不超过 `size` 字符，最多 `parts` 段。
pub fn sample_chunks(text: &str, size: usize, parts: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![];
    }
    if chars.len() <= size {
        return vec![text.trim().to_string()];
    }
    let n = (chars.len() / size).clamp(1, parts);
    let step = chars.len() / n;
    (0..n).map(|i| chars[i * step..(i * step + size).min(chars.len())].iter().collect::<String>()).collect()
}

/// 把新提取的术语并进待确认列表：已在启用术语库里的、已存在的不重复，重复出现的计数 +1。返回新增条数。
pub fn merge_candidates(data: &mut Data, found: Vec<Term>, site: &str, now: i64) -> usize {
    let known: HashSet<String> = data.glossaries.iter().flat_map(|g| flatten(&g.terms).into_iter().map(|t| t.source.to_lowercase())).collect();
    let mut added = 0;
    for t in found {
        let key = t.source.to_lowercase();
        if known.contains(&key) {
            continue;
        }
        let id = format!("{site}|{key}");
        if let Some(c) = data.candidates.iter_mut().find(|c| c.id == id) {
            c.count += 1;
            continue;
        }
        data.candidates.push(Candidate { id, source: t.source, target: t.target, note: t.note, site: site.to_string(), count: 1, found: now });
        added += 1;
    }
    if data.candidates.len() > MAX_CANDIDATES {
        let drop = data.candidates.len() - MAX_CANDIDATES;
        data.candidates.drain(..drop);
    }
    added
}

/// 纯文本里取出可读文字（去脚本、样式和标签）。
pub fn html_to_text(html: &str) -> String {
    use std::sync::LazyLock;
    static BLOCK: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?is)<script\b.*?</script\s*>|<style\b.*?</style\s*>|<noscript\b.*?</noscript\s*>|<svg\b.*?</svg\s*>|<head\b.*?</head\s*>").unwrap()
    });
    static BREAK: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?i)</?(p|div|br|li|h[1-6]|tr|section|article)\b[^>]*>").unwrap());
    static TAG: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?s)<[^>]*>").unwrap());
    let t = BLOCK.replace_all(html, " ");
    let t = BREAK.replace_all(&t, "\n");
    let t = TAG.replace_all(&t, " ");
    let t = t.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    t.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

// ---------- 社区术语库（沉浸式翻译 terms 仓库） ----------

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommunityItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub matches: Vec<String>,
    pub langs: Vec<String>,
}

fn pick_i18n(meta: &Value, key: &str) -> String {
    for lang in ["zh-CN", "zh", "en"] {
        if let Some(s) = meta.pointer(&format!("/i18ns/{lang}/{key}")).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    meta.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

pub fn parse_community_meta(meta: &Value) -> Option<CommunityItem> {
    let id = meta.get("id").and_then(Value::as_str)?.to_string();
    let strs = |k: &str| meta.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Some(CommunityItem {
        name: pick_i18n(meta, "name").chars().take(60).collect::<String>().trim().to_string(),
        description: pick_i18n(meta, "description"),
        author: meta.get("author").and_then(Value::as_str).unwrap_or("").to_string(),
        matches: strs("matches"),
        langs: strs("langs"),
        id,
    })
}

/// 在社区术语库支持的语言里，选最合适目标语言的 CSV 后缀；返回 (语言, 文件名)。
pub fn community_csv(item: &CommunityItem, tag: &str) -> Option<(String, String)> {
    let exact = item.langs.iter().find(|l| *l != "auto" && lang_ok(l, tag) && tag != "ai");
    if let Some(l) = exact {
        return Some((l.clone(), format!("{}_{}.csv", item.id, l)));
    }
    if item.langs.iter().any(|l| l == "auto") {
        return Some(("auto".into(), format!("{}.csv", item.id)));
    }
    item.langs.first().map(|l| (l.clone(), format!("{}_{}.csv", item.id, l)))
}

pub fn check_remote_id(id: &str) -> AppResult<()> {
    if !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !id.contains("..") {
        Ok(())
    } else {
        Err(AppError::invalid("术语库名称不合法。"))
    }
}

/// 让模型从文本里提取术语（长文本均匀取样，最多 4 段）。
pub async fn extract(ep: &crate::ai::Endpoint, model: &str, lang: &str, text: &str) -> AppResult<Vec<Term>> {
    let mut all = vec![];
    let system = extract_system(lang);
    for chunk in sample_chunks(text, 6000, 4) {
        let resp = crate::ai::chat(ep, model, &system, &chunk).await?;
        all.extend(parse_extracted(&resp));
    }
    Ok(clean_terms(all))
}

/// 提取结果进库：站点设了“直接加入某术语库”就加进去，否则进待确认。返回新增条数。
pub fn file_found(store: &Store, found: Vec<Term>, site_host: &str, now: i64) -> AppResult<usize> {
    store.edit(|d| {
        let target = d.sites.iter().find(|s| s.enabled && site_matches(&s.pattern, site_host)).map(|s| s.glossary.clone()).unwrap_or_default();
        let added = if let Some(g) = d.glossaries.iter_mut().find(|g| !target.is_empty() && g.id == target) {
            let have: HashSet<String> = flatten(&g.terms).into_iter().map(|t| t.source.to_lowercase()).collect();
            let fresh: Vec<Term> = found.into_iter().filter(|t| !have.contains(&t.source.to_lowercase())).collect();
            let n = fresh.len();
            g.terms.extend(fresh);
            g.terms = clean_terms(std::mem::take(&mut g.terms));
            g.updated = now;
            n
        } else {
            merge_candidates(d, found, site_host, now)
        };
        if let Some(s) = d.sites.iter_mut().find(|s| s.enabled && site_matches(&s.pattern, site_host)) {
            s.last = now;
            s.found = s.found.saturating_add(added as u32);
        }
        added
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str, tr: &str) -> Term {
        Term { source: s.into(), target: tr.into(), note: String::new(), ..Default::default() }
    }

    #[test]
    fn csv_roundtrip_with_quotes_bom_and_header() {
        let csv = "\u{feff}source,target,tgt_lng\r\nBlockchain,区块链,zh-CN\r\n\"Hello, world\",\"你好，\"\"世界\"\"\",zh-CN\r\n,空,zh-CN\r\nblockchain,重复,zh-CN\r\n";
        let terms = parse_csv(csv);
        assert_eq!(terms, vec![t("Blockchain", "区块链"), t("Hello, world", "你好，\"世界\"")]);
        let out = to_csv(&terms, "zh-CN");
        assert!(out.starts_with("source,target,tgt_lng,note,path,group\n"));
        assert_eq!(parse_csv(&out), terms);
        assert!(out.contains("\"Hello, world\""));
    }

    #[test]
    fn csv_without_header_or_tab_separated() {
        assert_eq!(parse_csv("API\tAPI 接口\nbug\t缺陷\n"), vec![t("API", "API 接口"), t("bug", "缺陷")]);
        assert_eq!(parse_csv("pitch,推介\n"), vec![t("pitch", "推介")]);
        let reordered = parse_csv("note,target,source\n游戏里的,存档,save point\n");
        assert_eq!(reordered, vec![Term { source: "save point".into(), target: "存档".into(), note: "游戏里的".into(), ..Default::default() }]);
    }

    #[test]
    fn site_patterns_follow_immersive_translate_rules() {
        assert!(site_matches("twitter.com", "https://twitter.com/a/b"));
        assert!(!site_matches("twitter.com", "https://x.twitter.com/"));
        assert!(site_matches("*.example.com", "https://a.example.com/x"));
        assert!(!site_matches("*.example.com", "https://example.com/x"), "*.域名 不含裸域名");
        assert!(site_matches("https://app.uniswap.org/*", "https://app.uniswap.org/swap?x=1"));
        assert!(!site_matches("https://app.uniswap.org/*", "https://uniswap.org/"));
        assert!(site_matches("*", "https://anything.org"));
        assert!(!site_matches("", "https://a.com"));
        assert!(site_matches("youtube.com", "youtube.com"), "只给域名也能匹配");
    }

    #[test]
    fn language_matching() {
        assert!(lang_ok("zh-CN", "zh") && !lang_ok("zh-TW", "zh"));
        assert!(lang_ok("zh-TW", "zh-Hant") && !lang_ok("zh-CN", "zh-Hant"));
        assert!(lang_ok("", "ja") && lang_ok("auto", "en"));
        assert!(lang_ok("en", "en") && !lang_ok("en", "ja"));
        assert!(lang_ok("zh-CN", "ai"));
    }

    #[test]
    fn selection_respects_enabled_site_lang_and_priority() {
        let mk = |id: &str, origin: &str, matches: &[&str], lang: &str, terms: Vec<Term>| Glossary {
            id: id.into(),
            name: id.into(),
            origin: origin.into(),
            matches: matches.iter().map(|s| s.to_string()).collect(),
            lang: lang.into(),
            terms,
            ..Default::default()
        };
        let list = vec![
            mk("com", "community", &[], "zh-CN", vec![t("Token", "令牌"), t("Gas", "燃料")]),
            mk("mine", "user", &[], "zh-CN", vec![t("token", "代币")]),
            mk("game", "user", &["store.steampowered.com"], "zh-CN", vec![t("Wishlist", "愿望单")]),
            mk("tw", "user", &[], "zh-TW", vec![t("Cache", "快取")]),
            Glossary { enabled: false, terms: vec![t("Off", "关")], ..mk("off", "user", &[], "", vec![]) },
        ];
        let on_steam = select_terms(&list, "https://store.steampowered.com/app/1", "zh");
        let srcs: Vec<_> = on_steam.iter().map(|x| (x.source.as_str(), x.target.as_str())).collect();
        assert_eq!(srcs, vec![("token", "代币"), ("Wishlist", "愿望单"), ("Gas", "燃料")], "自建优先于社区，同名(忽略大小写)只留一个");
        let elsewhere = select_terms(&list, "https://example.com", "zh");
        assert!(!elsewhere.iter().any(|x| x.source == "Wishlist" || x.source == "Cache"));
        assert!(select_terms(&list, "", "zh").iter().all(|x| x.source != "Wishlist"), "没有网址时不启用限定网站的术语库");
    }

    #[test]
    fn only_terms_present_in_the_batch_go_into_the_prompt() {
        let terms = vec![t("art", "艺术"), t("Machine learning", "机器学习"), t("GPU", "显卡"), t("原神", "Genshin")];
        let texts = vec!["We love the party and machine learning.".to_string(), "原神 is fun".to_string()];
        let hit = relevant(&terms, &texts, 10);
        let names: Vec<_> = hit.iter().map(|x| x.source.as_str()).collect();
        assert_eq!(names, vec!["Machine learning", "原神"], "art 不能命中 party；GPU 没出现");
        let block = prompt_block(&hit);
        assert!(block.contains("- Machine learning => 机器学习") && block.contains("- 原神 => Genshin"));
        assert_eq!(prompt_block(&[]), "");
        let keep = prompt_block(&[&t("iPhone", "")]);
        assert!(keep.contains("iPhone => (keep as is)"));
        assert_eq!(relevant(&terms, &texts, 1).len(), 1);
    }

    #[test]
    fn extracted_terms_parse_tolerantly() {
        let resp = "```json\n[{\"source\":\"Kubernetes\",\"target\":\"\",\"note\":\"keep\"},{\"term\":\"pod\",\"translation\":\"容器组\"},{\"source\":\"same\",\"target\":\"same\"},{\"source\":\"\",\"target\":\"x\"}]\n```";
        let got = parse_extracted(resp);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1], t("pod", "容器组"));
        assert!(parse_extracted("not json").is_empty());
    }

    #[test]
    fn candidates_skip_known_terms_and_count_repeats() {
        let mut d = Data { glossaries: vec![Glossary { terms: vec![t("Known", "已知")], ..Default::default() }], ..Default::default() };
        assert_eq!(merge_candidates(&mut d, vec![t("known", "x"), t("Fresh", "新")], "a.com", 1), 1);
        assert_eq!(merge_candidates(&mut d, vec![t("fresh", "新")], "a.com", 2), 0);
        assert_eq!(d.candidates.len(), 1);
        assert_eq!(d.candidates[0].count, 2);
        assert_eq!(merge_candidates(&mut d, vec![t("Fresh", "新")], "b.com", 3), 1, "不同网站分开记");
    }

    #[test]
    fn samples_and_html_text() {
        let long = "字".repeat(30_000);
        let parts = sample_chunks(&long, 6000, 4);
        assert_eq!(parts.len(), 4);
        assert!(parts.iter().all(|p| p.chars().count() <= 6000));
        assert_eq!(sample_chunks("short", 6000, 4), vec!["short"]);
        assert!(sample_chunks("", 10, 3).is_empty());
        let text = html_to_text(
            "<html><head><title>x</title><style>a{}</style></head><body><h1>Hello&nbsp;World</h1><script>var a=1</script><p>One <b>two</b></p></body></html>",
        );
        assert_eq!(text, "Hello World\nOne two");
    }

    #[test]
    fn community_meta_and_csv_choice() {
        let meta: Value = serde_json::from_str(r#"{"id":"game","name":"Game","description":"d","author":"immersive","langs":["auto","zh-CN","zh-TW"],"matches":["a.com"],"i18ns":{"zh-CN":{"name":"游戏","description":"游戏术语"}}}"#).unwrap();
        let item = parse_community_meta(&meta).unwrap();
        assert_eq!((item.name.as_str(), item.description.as_str()), ("游戏", "游戏术语"));
        assert_eq!(community_csv(&item, "zh"), Some(("zh-CN".into(), "game_zh-CN.csv".into())));
        assert_eq!(community_csv(&item, "zh-Hant"), Some(("zh-TW".into(), "game_zh-TW.csv".into())));
        assert_eq!(community_csv(&item, "ja"), Some(("auto".into(), "game.csv".into())));
        assert!(check_remote_id("../x").is_err() && check_remote_id("game").is_ok());
    }

    #[test]
    fn store_persists_and_cleans() {
        let dir = std::env::temp_dir().join(format!("cc-gl-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir);
        s.edit(|d| {
            let mut g = Glossary { name: "  ".into(), terms: vec![t("A", "甲"), t("a", "乙"), t(" ", "x")], ..Default::default() };
            clean_glossary(&mut g);
            d.glossaries.push(g);
        })
        .unwrap();
        let again = Store::open(&dir).snapshot();
        assert_eq!(again.glossaries[0].name, "未命名术语库");
        assert_eq!(again.glossaries[0].terms, vec![t("A", "甲")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn folders_are_cleaned_renamed_and_removed_without_losing_glossaries() {
        assert_eq!(clean_folder(" 游戏 / / 原神 /"), "游戏/原神");
        assert_eq!(clean_folder("a\\b/c/d/e/f/g/h"), "a/b/c/d/e/f");
        assert!(in_folder("游戏/原神", "游戏") && in_folder("游戏", "游戏") && !in_folder("游戏机", "游戏"));
        let g = |id: &str, f: &str| Glossary { id: id.into(), folder: f.into(), ..Default::default() };
        let mut d = Data {
            glossaries: vec![g("a", "游戏/原神"), g("b", "游戏"), g("c", "游戏机"), g("d", "")],
            folders: vec!["游戏/空的".into(), "工作".into()],
            ..Default::default()
        };
        rename_folder(&mut d, "游戏", "娱乐/游戏");
        let fs: Vec<_> = d.glossaries.iter().map(|x| x.folder.as_str()).collect();
        assert_eq!(fs, ["娱乐/游戏/原神", "娱乐/游戏", "游戏机", ""], "同名前缀（游戏机）不受影响");
        assert_eq!(d.folders, ["娱乐/游戏/空的", "工作"]);
        remove_folder(&mut d, "娱乐/游戏");
        let fs: Vec<_> = d.glossaries.iter().map(|x| x.folder.as_str()).collect();
        assert_eq!(fs, ["娱乐/原神", "娱乐", "游戏机", ""], "删文件夹不删术语库，往上挪一层");
        assert!(d.folders.contains(&"娱乐/空的".to_string()));
    }

    fn tree() -> Vec<Term> {
        let mut file = Term::new("文件", "File");
        file.note = "菜单名".into();
        file.children = vec![Term::new("打开文件", "Open File"), Term::new("退出", "Exit")];
        let mut edit = Term::new("编辑", "Edit");
        edit.children = vec![Term::new("复制", "Copy")];
        let mut ui = Term::new("界面", "Interface");
        ui.note = "软件界面".into();
        ui.children = vec![file, edit];
        let mut chars = Term::new("角色", "");
        chars.group = true;
        chars.children = vec![Term::new("旅行者", "Traveler")];
        vec![ui, chars]
    }

    #[test]
    fn nested_terms_flatten_with_their_path_and_groups_drop_out() {
        let flat = flatten(&tree());
        let srcs: Vec<&str> = flat.iter().map(|t| t.source.as_str()).collect();
        assert_eq!(srcs, ["界面", "文件", "打开文件", "退出", "编辑", "复制", "旅行者"], "只作分组的“角色”不当术语用");
        let open = flat.iter().find(|t| t.source == "打开文件").unwrap();
        assert_eq!(open.context, "in 界面 › 文件 = Interface › File; 软件界面; 菜单名");
        assert_eq!(flat[0].context, "", "第一层没有路径");
        // 分组没有译文时路径里用原文
        assert_eq!(flat.iter().find(|t| t.source == "旅行者").unwrap().context, "in 角色 = 角色");
        assert_eq!(count_terms(&tree()), 8);
    }

    #[test]
    fn prompt_shows_the_path_and_machine_translation_still_reads_it() {
        let flat = flatten(&tree());
        let hit = relevant(&flat, &["点打开文件".to_string()], 10);
        let block = prompt_block(&hit);
        assert!(block.contains("- 打开文件 => Open File  (in 界面 › 文件 = Interface › File; 软件界面; 菜单名)"), "{block}");
        assert!(block.contains("whose place fits"), "{block}");
        let pairs = crate::mt::glossary_of(&format!("x{block}"));
        assert!(pairs.contains(&("打开文件".to_string(), "Open File".to_string())), "{pairs:?}");
    }

    #[test]
    fn same_source_under_different_parents_is_kept_but_siblings_merge() {
        let mut a = Term::new("文件", "File");
        a.children = vec![Term::new("打开", "Open")];
        let mut b = Term::new("视图", "View");
        b.children = vec![Term::new("打开", "Show")];
        let dup = Term { source: "文件".into(), children: vec![Term::new("关闭", "Close")], ..Default::default() };
        let cleaned = clean_terms(vec![a, b, dup]);
        assert_eq!(cleaned.len(), 2, "同一层重名的合并");
        assert_eq!(cleaned[0].children.iter().map(|t| t.source.as_str()).collect::<Vec<_>>(), ["打开", "关闭"]);
        let g = Glossary { id: "g".into(), enabled: true, origin: "user".into(), terms: cleaned, ..Default::default() };
        let sel = select_terms(&[g], "", "en");
        assert_eq!(sel.iter().filter(|t| t.source == "打开").count(), 2, "不同上级下的同名术语都要");
    }

    #[test]
    fn depth_is_capped_and_empty_rows_lift_their_children() {
        let mut t = Term::new("L7", "");
        for i in (1..=6).rev() {
            t = Term { source: format!("L{i}"), children: vec![t], ..Default::default() };
        }
        let empty = Term { source: "  ".into(), children: vec![Term::new("孤儿", "Orphan")], ..Default::default() };
        let c = clean_terms(vec![t, empty]);
        let mut depth = 1;
        let mut cur = &c[0];
        while let Some(n) = cur.children.first() {
            depth += 1;
            cur = n;
        }
        assert_eq!(depth, MAX_DEPTH);
        assert_eq!(count_terms(&c), 8, "第 7 层提到第 6 层，没有丢");
        assert_eq!(c[1].source, "孤儿", "原文为空的行去掉，下级提上来");
    }

    #[test]
    fn csv_keeps_the_hierarchy_and_old_csv_stays_flat() {
        let out = to_csv(&tree(), "en");
        assert!(out.contains("打开文件,Open File,en,,界面/文件,\n"), "{out}");
        assert!(out.contains("角色,,en,,,1\n"), "{out}");
        assert_eq!(parse_csv(&out), clean_terms(tree()));
        // 只有 path 没有上级那一行：补一个分组
        let t = parse_csv("source,target,path\nRedo,重做,Edit\n");
        assert_eq!((t[0].source.as_str(), t[0].group, t[0].children[0].source.as_str()), ("Edit", true, "Redo"));
        // 沉浸式翻译的老格式：都在第一层
        assert_eq!(count_terms(&parse_csv("source,target,tgt_lng\nA,甲,zh\nB,乙,zh\n")), 2);
    }

    #[test]
    fn merging_follows_the_tree() {
        let mut have = tree();
        let mut ui = Term::new("界面", "UI");
        ui.children = vec![Term { source: "文件".into(), target: "File".into(), children: vec![Term::new("另存为", "Save As")], ..Default::default() }];
        merge_terms(&mut have, vec![ui]);
        assert_eq!(have[0].target, "UI", "同名的以新的为准");
        let file = &have[0].children[0];
        assert_eq!(file.children.iter().map(|t| t.source.as_str()).collect::<Vec<_>>(), ["打开文件", "退出", "另存为"]);
    }
}
