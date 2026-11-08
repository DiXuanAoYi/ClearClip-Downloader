//! 文件翻译的补充逻辑：页范围、不翻译的内容（页眉页脚 / 公式代码 / 参考文献）、开始前的估算、
//! 任务记录（重启后还在，能继续）、清译文缓存。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::filetr::{self, Kind, Opts};

// ---------- 页范围 ----------

/// `3-12, 15`、`5-`（从第 5 页到最后）→ 区间列表（从 1 起，含两端）。留空或一个都解析不出来返回 None（= 全部）。
pub fn parse_ranges(s: &str) -> Option<Vec<(usize, usize)>> {
    let mut out = vec![];
    for tok in s.split(|c: char| matches!(c, ',' | '，' | ';' | '；' | '、') || c.is_whitespace()).map(str::trim).filter(|t| !t.is_empty()) {
        let tok = tok.replace(['–', '—', '~', '～'], "-");
        let (a, b) = match tok.split_once('-') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (tok.as_str(), None),
        };
        let Ok(from) = a.parse::<usize>() else { continue };
        let to = match b {
            None => from,
            Some("") => usize::MAX,
            Some(b) => match b.parse::<usize>() {
                Ok(t) => t,
                Err(_) => continue,
            },
        };
        if from >= 1 && to >= from {
            out.push((from, to));
        }
    }
    (!out.is_empty()).then_some(out)
}

pub fn in_ranges(r: &[(usize, usize)], page1: usize) -> bool {
    r.iter().any(|(a, b)| (*a..=*b).contains(&page1))
}

// ---------- 不翻译的内容 ----------

static PAGE_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[\s\-–—·•]*(?:page\s*)?\d{1,4}(?:\s*(?:/|of)\s*\d{1,4})?[\s\-–—·•]*$|^\s*第\s*\d+\s*页(?:\s*[/／]?\s*共?\s*\d+\s*页)?\s*$").unwrap()
});
static URL_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:https?://|www\.)\S+$|^[\w.+-]+@[\w-]+(?:\.[\w-]+)+$").unwrap());

/// 页码一类（“12”、“Page 3 of 10”、“第 3 页”）。
pub fn page_number_like(t: &str) -> bool {
    PAGE_NUM.is_match(t.trim())
}

/// 公式、代码、网址这类翻了没意义的片段：整段是网址 / 邮箱；字母占的比例很低；或者没有一个三个字母以上的词（`x_i = Wx`、`O(n^2)`）。
pub fn untranslatable(t: &str) -> bool {
    let t = t.trim();
    if t.is_empty() {
        return true;
    }
    if URL_ONLY.is_match(t) {
        return true;
    }
    // 有汉字、假名、韩文的肯定是文字
    if t.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c)) {
        return false;
    }
    let non_space = t.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    if (letters as f64) / (non_space as f64) < 0.3 {
        return true;
    }
    t.chars().count() <= 60 && !t.split(|c: char| !c.is_alphabetic()).any(|w| w.chars().count() >= 3)
}

/// 参考文献的标题行。
pub fn is_refs_heading(t: &str) -> bool {
    let k = t.trim().trim_end_matches([':', '：', '.']).trim().to_lowercase();
    matches!(k.as_str(), "references" | "reference" | "bibliography" | "works cited" | "参考文献" | "参考资料" | "参考书目" | "参 考 文 献")
}

fn norm_header(t: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in t.trim().chars() {
        if c.is_ascii_digit() {
            if !out.ends_with('#') {
                out.push('#');
            }
            last_space = false;
        } else if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.extend(c.to_lowercase());
            last_space = false;
        }
    }
    out
}

/// 哪些页要翻译、哪些片段按设置不翻译。
/// `seg_page[i]` 是第 i 个片段所在的（阅读器里的）页序号；`page_no[pi]` 是这一页对应 PDF 的第几页（版面翻译时才有）。
/// 返回：每一页要不要翻译（页范围里的），以及不翻译的片段序号。页范围按 PDF 页号（没有时按序号）算，从 1 起。
pub fn select_and_skip(
    texts: &[String],
    seg_page: &[usize],
    page_no: &[Option<usize>],
    opts: &Opts,
    pdf_layout: bool,
    is_sub: bool,
) -> (Vec<bool>, HashSet<usize>) {
    let ranges = parse_ranges(&opts.pages);
    let selected: Vec<bool> = page_no.iter().enumerate().map(|(pi, p)| ranges.as_ref().map_or(true, |r| in_ranges(r, p.unwrap_or(pi) + 1))).collect();
    let mut skip: HashSet<usize> = HashSet::new();
    if opts.skip_code {
        for (i, t) in texts.iter().enumerate() {
            if untranslatable(t) {
                skip.insert(i);
            }
        }
    }
    if opts.skip_headers && pdf_layout {
        // 同一句话（数字不算）在三页以上都出现、又不长、又在页面的头两块或最后两块：页眉页脚
        // （正文里“Table 1 / Table 2 / Table 3”这样只差数字的短句在页面中间，不算）
        let mut page_len: HashMap<usize, usize> = HashMap::new();
        for p in seg_page {
            *page_len.entry(*p).or_default() += 1;
        }
        let mut seen_in_page: HashMap<usize, usize> = HashMap::new();
        let edge: Vec<bool> = seg_page
            .iter()
            .map(|p| {
                let k = seen_in_page.entry(*p).or_default();
                let at = *k;
                *k += 1;
                at < 2 || at + 2 >= page_len[p]
            })
            .collect();
        let mut pages_of: HashMap<String, HashSet<usize>> = HashMap::new();
        for (i, t) in texts.iter().enumerate() {
            if edge[i] && t.chars().count() <= 80 {
                pages_of.entry(norm_header(t)).or_default().insert(seg_page[i]);
            }
        }
        for (i, t) in texts.iter().enumerate() {
            if edge[i] && (page_number_like(t) || (t.chars().count() <= 80 && pages_of.get(&norm_header(t)).is_some_and(|p| p.len() >= 3))) {
                skip.insert(i);
            }
        }
    }
    if opts.skip_refs && !is_sub {
        if let Some(at) = texts.iter().position(|t| is_refs_heading(t)) {
            skip.extend(at..texts.len());
        }
    }
    (selected, skip)
}

// ---------- 开始前的估算 ----------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    /// 文件一共几页（PDF 的页数；EPUB 的章数；字幕按 40 条一页）
    pub pages: usize,
    /// 页范围里的页数
    pub selected_pages: usize,
    /// 要翻译的片段数（页范围里、没被“不翻译”排除的）
    pub segments: usize,
    pub chars: usize,
    /// 其中缓存里已经有译文的
    pub cached: usize,
    /// 按设置不翻译的片段数
    pub skipped: usize,
    /// 预计向模型发几次请求（已缓存的不算）
    pub requests: usize,
    /// 扫描版（没有文字层）：要先识字，没法在这里数出片段
    pub scanned: bool,
    /// 按 PDF 版面的块切（装了排版组件）
    pub layout: bool,
    /// 缓存里已经处理过的页数
    pub done_pages: usize,
}

pub async fn plan(st: &crate::AppState, input: &Path, opts: &Opts) -> AppResult<PlanInfo> {
    let kind = filetr::kind_of(input).ok_or_else(|| AppError::invalid("不支持这种文件。"))?;
    let settings = st.settings();
    let target = if opts.target.trim().is_empty() { settings.ai.target_lang.clone() } else { opts.target.trim().to_string() };
    let mut info = PlanInfo::default();
    // (每页的片段, 每页对应的 PDF 页号)
    let mut page_texts: Vec<Vec<String>> = vec![];
    let mut page_no: Vec<Option<usize>> = vec![];
    let layout_lib = if kind == Kind::Pdf && opts.pdf_mode != "txt" { crate::pdfx::find_lib(&st.data_dir) } else { None };
    let mut done_layout = false;
    if let Some(lib) = layout_lib {
        let inp = input.to_path_buf();
        let pages = tokio::task::spawn_blocking(move || crate::pdfx::extract(&lib, &inp)).await.map_err(|e| AppError::msg(e.to_string()))??;
        if pages.iter().any(|p| !p.blocks.is_empty()) {
            info.pages = pages.len();
            for (i, p) in pages.iter().enumerate().filter(|(_, p)| !p.blocks.is_empty()) {
                page_texts.push(p.blocks.iter().map(|b| b.text.clone()).collect());
                page_no.push(Some(i));
            }
            info.layout = true;
            done_layout = true;
        } else if pages.iter().all(|p| !p.has_text) {
            info.pages = pages.len();
            info.selected_pages = pages.len();
            info.scanned = true;
            return Ok(info);
        }
    }
    if !done_layout {
        let v = filetr::preview(input)?;
        info.pages = v.pages.len();
        for p in &v.pages {
            page_texts.push(p.src.clone());
            page_no.push(None);
        }
        if kind == Kind::Img || (kind == Kind::Pdf && v.pages.is_empty()) {
            info.scanned = true;
            info.selected_pages = info.pages.max(1);
            return Ok(info);
        }
    }
    let texts: Vec<String> = page_texts.iter().flatten().cloned().collect();
    let seg_page: Vec<usize> = page_texts.iter().enumerate().flat_map(|(pi, p)| std::iter::repeat(pi).take(p.len())).collect();
    let (selected, skip) = select_and_skip(&texts, &seg_page, &page_no, opts, info.layout, kind == Kind::Sub);
    info.selected_pages = selected.iter().filter(|s| **s).count();
    let cache = filetr::load_cache(&filetr::cache_path(&st.data_dir, input, &target));
    let cached_trans = cache.as_ref().map(|v| filetr::fill_from_cache(v, &texts));
    info.done_pages = cache.as_ref().map_or(0, |v| v.pages.iter().filter(|p| p.done).count());
    for (i, t) in texts.iter().enumerate() {
        if skip.contains(&i) {
            if selected[seg_page[i]] {
                info.skipped += 1;
            }
            continue;
        }
        if !selected[seg_page[i]] {
            continue;
        }
        info.segments += 1;
        info.chars += t.chars().count();
        if cached_trans.as_ref().is_some_and(|c| c[i].is_some()) {
            info.cached += 1;
        }
    }
    let batch = settings.ai.batch_size.clamp(8, 30);
    info.requests = (info.segments - info.cached).div_ceil(batch);
    Ok(info)
}

// ---------- 任务记录：重启后还在 ----------

/// 一条文件翻译任务的记录（和“进行中的任务”分开存：任务列表只在内存里，重启就没了）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Recent {
    pub path: String,
    pub title: String,
    /// 目标语言（已经解析好的，不会是空）
    pub target: String,
    pub opts: Opts,
    /// 开始时间（Unix 秒）
    pub at: i64,
    /// 生成出来的文件
    pub output: Option<String>,
    /// 上一次的结果：空 = 还没结束（或程序被关了）/ ok / cancel / err
    pub last: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentView {
    #[serde(flatten)]
    pub rec: Recent,
    /// stopped 已停止 / failed 失败 / ready 已译完待生成 / generated 已生成
    pub state: String,
    pub pages_total: usize,
    pub pages_done: usize,
    pub failed: usize,
    /// 源文件不在了
    pub missing: bool,
}

fn recents_path(data_dir: &Path) -> PathBuf {
    data_dir.join("filetr").join("tasks.json")
}

fn read_recents(data_dir: &Path) -> Vec<Recent> {
    std::fs::read(recents_path(data_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_recents(data_dir: &Path, list: &[Recent]) {
    let p = recents_path(data_dir);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec(list) {
        let _ = std::fs::write(p, b);
    }
}

/// 新增或更新一条记录（按文件 + 目标语言）；最多留 60 条，最新的在前。
pub fn recent_put(data_dir: &Path, rec: Recent) {
    let mut list = read_recents(data_dir);
    list.retain(|r| !(r.path == rec.path && r.target == rec.target));
    list.insert(0, rec);
    list.truncate(60);
    write_recents(data_dir, &list);
}

/// 改一条记录的结果。
pub fn recent_finish(data_dir: &Path, path: &str, target: &str, last: &str, output: Option<String>, error: &str) {
    let mut list = read_recents(data_dir);
    if let Some(r) = list.iter_mut().find(|r| r.path == path && r.target == target) {
        r.last = last.to_string();
        if output.is_some() {
            r.output = output;
        }
        r.error = error.to_string();
        write_recents(data_dir, &list);
    }
}

pub fn recent_remove(data_dir: &Path, path: &str, target: &str) {
    let mut list = read_recents(data_dir);
    let n = list.len();
    list.retain(|r| !(r.path == path && r.target == target));
    if list.len() != n {
        write_recents(data_dir, &list);
    }
}

pub fn recent_list(data_dir: &Path) -> Vec<RecentView> {
    read_recents(data_dir)
        .into_iter()
        .map(|rec| {
            let view = filetr::load_cache(&filetr::cache_path(data_dir, Path::new(&rec.path), &rec.target));
            let (pages_total, pages_done, failed) = view.as_ref().map_or((0, 0, 0), |v| (v.pages.len(), v.pages.iter().filter(|p| p.done).count(), v.failed));
            let out_ok = rec.output.as_deref().is_some_and(|o| Path::new(o).exists());
            let state = match rec.last.as_str() {
                "ok" if rec.opts.preview || !out_ok => "ready",
                "ok" => "generated",
                "err" => "failed",
                _ => "stopped",
            }
            .to_string();
            let missing = !Path::new(&rec.path).exists();
            RecentView { rec, state, pages_total, pages_done, failed, missing }
        })
        .collect()
}

// ---------- 导出成文件 ----------

/// 按缓存里的译文生成文本：`txt` 只要译文（没译的保留原文）/ `md` 双语 Markdown / `srt`、`srt-trans` 字幕（双语 / 仅译文）。
pub fn export_text(data_dir: &Path, input: &Path, target: &str, kind: &str) -> AppResult<String> {
    let v = filetr::load_cache(&filetr::cache_path(data_dir, input, target)).ok_or_else(|| AppError::not_found("这个文件还没有译文，先翻译。"))?;
    match kind {
        "txt" | "md" => {
            let mut out = String::new();
            for (pi, p) in v.pages.iter().enumerate() {
                if kind == "md" && v.pages.len() > 1 {
                    out.push_str(&format!("## 第 {} 页\n\n", p.page.unwrap_or(pi) + 1));
                }
                for (i, src) in p.src.iter().enumerate() {
                    let t = p.trans.get(i).and_then(|t| t.as_deref()).filter(|t| !t.trim().is_empty());
                    if kind == "md" {
                        out.push_str(&format!("> {}\n\n{}\n\n", src.replace('\n', "\n> "), t.unwrap_or("（未译）")));
                    } else {
                        out.push_str(t.unwrap_or(src));
                        out.push_str("\n\n");
                    }
                }
            }
            Ok(out.trim_end().to_string() + "\n")
        }
        "srt" | "srt-trans" => {
            if filetr::kind_of(input) != Some(Kind::Sub) {
                return Err(AppError::invalid("只有字幕文件能导出 SRT。"));
            }
            let cues = crate::subtitle_tools::read(input)?;
            let flat: Vec<Option<String>> = v.pages.iter().flat_map(|p| p.trans.iter().cloned()).collect();
            if flat.len() != cues.len() {
                return Err(AppError::invalid("译文和字幕条数对不上（字幕文件改过？），重新翻译一次。"));
            }
            let d = if kind == "srt" { crate::subgen::Display::OrigTop } else { crate::subgen::Display::Trans };
            Ok(crate::subtitle::format_srt(&crate::subgen::compose(&cues, &flat, d)))
        }
        _ => Err(AppError::invalid("不支持的导出格式")),
    }
}

// ---------- 清缓存 ----------

/// 删掉某个文件（某个目标语言）的译文缓存：阅读器数据、扫描页的位置、识字结果。返回删了几个文件。
pub fn delete_cache(data_dir: &Path, input: &Path, target: &str) -> usize {
    let mut n = 0;
    let mut rm = |p: PathBuf| {
        if std::fs::remove_file(&p).is_ok() {
            n += 1;
        }
    };
    rm(filetr::cache_path(data_dir, input, target));
    for engine in ["vision", "enhanced", "system", "api"] {
        rm(filetr::scan_path(data_dir, input, target, engine));
        rm(filetr::ocr_path(data_dir, input, engine));
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_translated_text_and_markdown() {
        use crate::filetr::{save_cache, View, ViewPage};
        let dir = std::env::temp_dir().join(format!("cc-filetr-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("a.txt");
        std::fs::write(&input, "x").unwrap();
        let v = View {
            pages: vec![
                ViewPage { src: vec!["Hello".into(), "World".into()], trans: vec![Some("你好".into()), None], done: true, ..Default::default() },
                ViewPage { src: vec!["Bye".into()], trans: vec![Some("再见".into())], done: true, ..Default::default() },
            ],
            ..Default::default()
        };
        save_cache(&filetr::cache_path(&dir, &input, "简体中文"), &v);
        assert_eq!(export_text(&dir, &input, "简体中文", "txt").unwrap(), "你好\n\nWorld\n\n再见\n", "没译的保留原文");
        let md = export_text(&dir, &input, "简体中文", "md").unwrap();
        assert!(md.contains("## 第 1 页") && md.contains("> Hello\n\n你好") && md.contains("（未译）") && md.contains("## 第 2 页"), "{md}");
        assert!(export_text(&dir, &input, "简体中文", "srt").is_err(), "不是字幕");
        assert!(export_text(&dir, &input, "English", "txt").is_err(), "没有这个语言的译文");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_ranges_parse_leniently() {
        assert_eq!(parse_ranges("3-12, 15"), Some(vec![(3, 12), (15, 15)]));
        assert_eq!(parse_ranges("5-"), Some(vec![(5, usize::MAX)]));
        assert_eq!(parse_ranges("１,2～4；7"), Some(vec![(2, 4), (7, 7)]), "全角数字认不出来就跳过，别的照常");
        assert_eq!(parse_ranges(""), None);
        assert_eq!(parse_ranges("abc, 0, 9-3"), None, "全是坏的 = 当作没写（全部）");
        let r = parse_ranges("3-5, 9-").unwrap();
        assert!(in_ranges(&r, 4) && in_ranges(&r, 100) && !in_ranges(&r, 6) && !in_ranges(&r, 2));
    }

    #[test]
    fn untranslatable_text_is_recognised() {
        for t in ["https://example.com/a/b", "name@example.org", "x_i = Wx", "O(n^2)", "3.14159 × 10^-3", "= 0.82 ± 0.04"] {
            assert!(untranslatable(t), "{t}");
        }
        for t in ["Attention is all you need", "We use it.", "注意力机制", "Fig. 3 shows the results of our model", "こんにちは"] {
            assert!(!untranslatable(t), "{t}");
        }
    }

    #[test]
    fn page_numbers_and_refs_headings() {
        for t in ["12", " - 3 - ", "Page 3 of 10", "page 7", "第 3 页", "第3页 共10页", "4 / 20"] {
            assert!(page_number_like(t), "{t}");
        }
        for t in ["Chapter 12", "2023 results", "Table 1"] {
            assert!(!page_number_like(t), "{t}");
        }
        assert!(is_refs_heading("References") && is_refs_heading("参考文献：") && is_refs_heading(" BIBLIOGRAPHY "));
        assert!(!is_refs_heading("References and notes on the method"));
    }

    fn opts() -> Opts {
        Opts::default()
    }

    #[test]
    fn headers_footers_refs_and_ranges_are_skipped() {
        // 4 页，每页：重复的页眉 + 页码 + 正文；最后一页有参考文献
        let mut texts = vec![];
        let mut seg_page = vec![];
        for p in 0..4 {
            texts.push("Journal of Examples, Vol. 12".to_string());
            seg_page.push(p);
            texts.push(format!("Opening paragraph of page {p} discusses the topic {} in some detail.", ["alpha", "beta", "gamma", "delta"][p]));
            seg_page.push(p);
            texts.push(format!("Table {}", p + 1)); // 只差数字、在页面中间：不是页眉
            seg_page.push(p);
            texts.push(format!("Closing paragraph of page {p} wraps up the topic {} for readers.", ["north", "south", "east", "west"][p]));
            seg_page.push(p);
            texts.push(format!("{}", p + 1));
            seg_page.push(p);
        }
        texts.push("References".to_string());
        seg_page.push(3);
        texts.push("Smith, J. (2020). A study of things. Journal of Stuff.".to_string());
        seg_page.push(3);
        let page_no: Vec<Option<usize>> = (0..4).map(Some).collect();

        let (sel, skip) = select_and_skip(&texts, &seg_page, &page_no, &opts(), true, false);
        assert_eq!(sel, vec![true; 4]);
        assert!([0, 5, 10, 15].iter().all(|i| skip.contains(i)), "重复的页眉：{skip:?}");
        assert!([4, 9, 14, 19].iter().all(|i| skip.contains(i)), "页码：{skip:?}");
        assert!([2, 7, 12, 17].iter().all(|i| !skip.contains(i)), "页面中间只差数字的短句（Table 1…）不是页眉：{skip:?}");
        assert!(!skip.contains(&1) && !skip.contains(&3) && !skip.contains(&20) && !skip.contains(&21), "正文和参考文献默认还要译：{skip:?}");

        let o = Opts { skip_refs: true, pages: "2-3".into(), ..opts() };
        let (sel, skip) = select_and_skip(&texts, &seg_page, &page_no, &o, true, false);
        assert_eq!(sel, vec![false, true, true, false], "页范围按 PDF 页号（从 1 起）");
        assert!(skip.contains(&20) && skip.contains(&21), "从 References 起都不译");

        let o = Opts { skip_headers: false, skip_code: false, ..opts() };
        let (_, skip) = select_and_skip(&texts, &seg_page, &page_no, &o, true, false);
        assert!(skip.is_empty(), "都关掉就都译：{skip:?}");

        // 字幕没有页眉页脚这回事，也不按“参考文献”截断
        let (_, skip) = select_and_skip(&texts, &seg_page, &page_no, &Opts { skip_refs: true, ..opts() }, false, true);
        assert!(!skip.contains(&20));
    }

    #[test]
    fn recents_survive_and_report_state() {
        let dir = std::env::temp_dir().join(format!("cc-filetr-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("paper.pdf");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&src, b"%PDF").unwrap();
        let rec = |last: &str| Recent {
            path: src.to_string_lossy().into_owned(),
            title: "paper".into(),
            target: "简体中文".into(),
            at: 1,
            last: last.into(),
            ..Default::default()
        };
        recent_put(&dir, rec(""));
        assert_eq!(recent_list(&dir)[0].state, "stopped", "没结束 = 已停止，可以继续");
        recent_finish(&dir, &src.to_string_lossy(), "简体中文", "err", None, "模型没有返回译文");
        let l = recent_list(&dir);
        assert_eq!((l[0].state.as_str(), l[0].rec.error.as_str()), ("failed", "模型没有返回译文"));
        recent_finish(&dir, &src.to_string_lossy(), "简体中文", "ok", None, "");
        assert_eq!(recent_list(&dir)[0].state, "ready", "没有生成文件 = 待生成");
        let out = dir.join("paper.译文.pdf");
        std::fs::write(&out, b"x").unwrap();
        recent_finish(&dir, &src.to_string_lossy(), "简体中文", "ok", Some(out.to_string_lossy().into_owned()), "");
        assert_eq!(recent_list(&dir)[0].state, "generated");
        // 同一个文件、同一语言只留一条
        recent_put(&dir, rec(""));
        assert_eq!(recent_list(&dir).len(), 1);
        std::fs::remove_file(&src).unwrap();
        assert!(recent_list(&dir)[0].missing);
        recent_remove(&dir, &src.to_string_lossy(), "简体中文");
        assert!(recent_list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_is_reused_by_text_when_the_order_changed() {
        use crate::filetr::{fill_from_cache, View, ViewPage};
        let old = View {
            pages: vec![ViewPage {
                src: vec!["a".into(), "b".into(), "c".into()],
                trans: vec![Some("甲".into()), None, Some("丙".into())],
                ..Default::default()
            }],
            ..Default::default()
        };
        // 顺序和内容一样：一一对应
        assert_eq!(fill_from_cache(&old, &["a".into(), "b".into(), "c".into()]), vec![Some("甲".into()), None, Some("丙".into())]);
        // 换了切法：按原文找
        assert_eq!(fill_from_cache(&old, &["c".into(), "x".into(), "a".into()]), vec![Some("丙".into()), None, Some("甲".into())]);
    }
}
