//! 给文字找一个能显示中文的字体文件。ffmpeg 的 `drawtext` 在没有 fontconfig 的版本里（比如 Windows 版）必须明确给出字体文件。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 按优先级排列的字体文件名（小写）：中文字体在前，西文字体兜底。
const PREFERRED: &[&str] = &[
    "msyh.ttc",
    "msyh.ttf",
    "pingfang.ttc",
    "notosanscjk-regular.ttc",
    "notosanscjksc-regular.otf",
    "notosanssc-regular.otf",
    "notosanssc-regular.ttf",
    "sourcehansanssc-regular.otf",
    "wqy-microhei.ttc",
    "wqy-zenhei.ttc",
    "simhei.ttf",
    "simsun.ttc",
    "stheiti medium.ttc",
    "hiragino sans gb.ttc",
    "droidsansfallbackfull.ttf",
    "arial unicode.ttf",
    "arialuni.ttf",
    "arial.ttf",
    "dejavusans.ttf",
    "helvetica.ttc",
];

fn font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut v: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        let win = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        v.push(win.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(l).join(r"Microsoft\Windows\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            v.push(h.join("Library/Fonts"));
        }
    } else {
        v.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            v.push(h.join(".fonts"));
            v.push(h.join(".local/share/fonts"));
        }
    }
    v
}

fn is_font(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("ttf" | "otf" | "ttc"))
}

/// 递归列出文件夹里的字体文件（限制层数和数量，避免扫太久）。
fn scan(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 4 || out.len() > 20_000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(&p, depth + 1, out);
        } else if is_font(&p) {
            out.push(p);
        }
    }
}

/// 从一批字体文件里挑默认字体：先按优先级找名字，找不到再找名字里带 cjk / hei 的。
pub fn pick(files: &[PathBuf]) -> Option<PathBuf> {
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    for want in PREFERRED {
        if let Some(p) = files.iter().find(|p| name(p) == *want) {
            return Some(p.clone());
        }
    }
    files
        .iter()
        .find(|p| {
            let n = name(p);
            n.contains("cjk") || n.contains("hei") || n.contains("yahei")
        })
        .cloned()
}

/// 系统里能用的默认字体（只找一次）。
pub fn default_font() -> Option<PathBuf> {
    static FONT: OnceLock<Option<PathBuf>> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut files = vec![];
        for d in font_dirs() {
            scan(&d, 0, &mut files);
        }
        pick(&files)
    })
    .clone()
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn be32(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// 字体文件里的家族名（`name` 表的 16 号，没有就 1 号）。ASS 字幕靠名字而不是路径找字体。读不出来返回 `None`。
pub fn family_name(path: &Path) -> Option<String> {
    let b = std::fs::read(path).ok()?;
    // .ttc：取第一个字体
    let base = if b.get(0..4)? == b"ttcf" { be32(&b, 12)? } else { 0 };
    let tables = be16(&b, base + 4)?;
    let mut name_at = None;
    for i in 0..tables {
        let rec = base + 12 + i * 16;
        if b.get(rec..rec + 4)? == b"name" {
            name_at = Some(be32(&b, rec + 8)?);
        }
    }
    let t = name_at?;
    let count = be16(&b, t + 2)?;
    let strings = t + be16(&b, t + 4)?;
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = t + 6 + i * 12;
        let (platform, lang, id, len, off) = (be16(&b, r)?, be16(&b, r + 4)?, be16(&b, r + 6)?, be16(&b, r + 8)?, be16(&b, r + 10)?);
        if id != 1 && id != 16 {
            continue;
        }
        let raw = b.get(strings + off..strings + off + len)?;
        let text = match platform {
            3 | 0 => String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()),
            1 => raw.iter().map(|&c| c as char).collect(),
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        // 16 号优先，其次英文
        let rank = (if id == 16 { 2 } else { 0 }) + u8::from(lang == 0x409 || platform == 1);
        if best.as_ref().map_or(true, |(r, _)| rank > *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn chinese_fonts_come_before_latin_ones() {
        let files = vec![p("/f/DejaVuSans.ttf"), p("/f/NotoSansCJK-Regular.ttc"), p("/f/arial.ttf")];
        assert_eq!(pick(&files), Some(p("/f/NotoSansCJK-Regular.ttc")));
        let files = vec![p("/f/arial.ttf"), p("/f/MSYH.TTC")];
        assert_eq!(pick(&files), Some(p("/f/MSYH.TTC")), "大小写不敏感");
        assert_eq!(pick(&[p("/f/Roboto.ttf")]), None);
        assert_eq!(pick(&[p("/f/Roboto.ttf"), p("/f/SomeCJKFont.otf")]), Some(p("/f/SomeCJKFont.otf")));
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn family_name_is_read_from_the_font_file() {
        let f = p("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
        if f.is_file() {
            assert_eq!(family_name(&f).as_deref(), Some("DejaVu Sans"));
        }
        assert_eq!(family_name(&p("/definitely/not/here.ttf")), None);
        let junk = std::env::temp_dir().join(format!("clearclip-junk-{}.ttf", std::process::id()));
        std::fs::write(&junk, b"not a font").unwrap();
        assert_eq!(family_name(&junk), None);
        let _ = std::fs::remove_file(junk);
    }

    #[test]
    fn only_font_files_are_listed() {
        let d = std::env::temp_dir().join(format!("clearclip-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub/deeper")).unwrap();
        for f in ["a.ttf", "sub/b.OTF", "sub/deeper/c.ttc", "sub/readme.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let mut out = vec![];
        scan(&d, 0, &mut out);
        assert_eq!(out.len(), 3);
        let _ = std::fs::remove_dir_all(d);
    }
}
