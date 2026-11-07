//! 图片识字统一入口：截图翻译（桌面截图窗、扩展截图）、文件翻译里的图片 / 扫描页、屏幕翻译都用同一套
//! “识字引擎 → 按版面并段 → 翻译 → 每段在图上的相对位置”。
//!
//! 识字引擎：
//! - `vision`：视觉大模型一步给出原文、译文和位置（imgtrans.rs）
//! - `enhanced` / `system` / `api`：本机增强识字 / Windows 自带识字 / 识字接口，位置是像素级的；译文交给翻译任务（术语库、AI 专家、翻译记忆）
//! - `local`：用“设置 → 屏幕翻译”里选的那个本机识字方式（选的是视觉模型时退到增强识字）

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine as _;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::imgtrans::Block;
use crate::livecore::{group_lines, Group, OLine};

/// 识字方式是否走本机（不是视觉模型）。
pub fn is_local(engine: &str) -> bool {
    matches!(engine, "enhanced" | "system" | "api" | "local")
}

/// `local` → 具体的本机引擎。
pub fn resolve(engine: &str, live_ocr: &str) -> &'static str {
    match engine {
        "enhanced" => "enhanced",
        "system" => "system",
        "api" => "api",
        _ => match live_ocr {
            "system" => "system",
            "api" => "api",
            _ => "enhanced",
        },
    }
}

/// data URL（PNG / JPEG / WebP）→ BGRA 像素。
pub fn decode_data_url(url: &str) -> AppResult<(Vec<u8>, usize, usize)> {
    crate::imgtrans::check_image(url)?;
    let b64 = url.split_once(',').map(|x| x.1).unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| AppError::invalid("图片数据不对"))?;
    crate::filetr::decode_bgra(&bytes)
}

/// 共用的版面计算：识字得到的行 → 段落 → 0–1 的相对位置（和视觉模型给的位置同一种格式，界面按同一套方法画）。
pub fn blocks_of(groups: &[Group], translations: &[String], w: usize, h: usize) -> Vec<Block> {
    let (fw, fh) = (w.max(1) as f64, h.max(1) as f64);
    groups
        .iter()
        .zip(translations.iter().map(Some).chain(std::iter::repeat(None)))
        .filter(|(g, _)| !g.text.trim().is_empty())
        .map(|(g, tr)| {
            let x = (g.x as f64 / fw).clamp(0.0, 1.0);
            let y = (g.y as f64 / fh).clamp(0.0, 1.0);
            let rw = (g.w as f64 / fw).clamp(0.0, 1.0 - x);
            let rh = (g.h as f64 / fh).clamp(0.0, 1.0 - y);
            Block {
                text: g.text.trim().to_string(),
                translation: tr.map(|t| t.trim().to_string()).unwrap_or_default(),
                rect: (rw > 0.0 && rh > 0.0).then_some([x, y, rw, rh]),
            }
        })
        .collect()
}

/// 用本机引擎识一张图（在阻塞线程里跑）。
pub async fn recognize(st: &crate::AppState, engine: &str, bgra: Vec<u8>, w: usize, h: usize) -> AppResult<Vec<OLine>> {
    let (dir, client, live, engine) = (st.data_dir.clone(), st.dl_client.clone(), st.settings().live.clone(), engine.to_string());
    tokio::task::spawn_blocking(move || {
        let cx = crate::ocr::Ctx { data_dir: &dir, client: &client, live: &live };
        crate::ocr::recognize(&cx, &engine, &bgra, w, h)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

/// 本机识字 + 翻译任务：返回和视觉模型一样的 `{"blocks": [...], "engine": ...}`。
pub async fn translate_image(
    app: &tauri::AppHandle,
    st: &Arc<crate::AppState>,
    req: &crate::vsum::Req,
    stop: &AtomicBool,
    progress: impl Fn(usize, usize),
) -> AppResult<Value> {
    let engine = resolve(req.ocr.trim(), &st.settings().live.ocr);
    let (bgra, w, h) = decode_data_url(&req.image)?;
    progress(0, 2);
    let lines = recognize(st, engine, bgra.clone(), w, h).await?;
    let lines = if st.settings().live.skip_icons { crate::livecore::drop_icons(lines, Some((&bgra, w, h))) } else { lines };
    let groups = group_lines(&lines);
    if stop.load(Ordering::Relaxed) {
        return Err(AppError::msg("已取消"));
    }
    progress(1, 2);
    if groups.is_empty() {
        return Ok(json!({"blocks": [], "engine": engine}));
    }
    let tr_req: crate::vsum::Req = serde_json::from_value(json!({
        "kind": "webtrans",
        "lang": req.lang,
        "title": req.title,
        "url": req.url,
        "context": "Text recognized (OCR) from a screenshot: paragraphs, labels, buttons, captions. OCR may contain small recognition errors; read through them.",
        "lines": groups.iter().enumerate().map(|(i, g)| json!({"t": i, "text": g.text})).collect::<Vec<_>>(),
    }))
    .map_err(|e| AppError::msg(e.to_string()))?;
    let res = crate::vsum::run_now(app, tr_req, stop).await?;
    let got: std::collections::HashMap<u64, String> =
        res["lines"].as_array().into_iter().flatten().filter_map(|l| Some((l["t"].as_u64()?, l["text"].as_str()?.to_string()))).collect();
    let trs: Vec<String> = (0..groups.len()).map(|i| got.get(&(i as u64)).cloned().unwrap_or_default()).collect();
    progress(2, 2);
    Ok(json!({"blocks": blocks_of(&groups, &trs, w, h), "engine": engine}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_engine_names() {
        assert!(is_local("enhanced") && is_local("local") && is_local("system") && is_local("api"));
        assert!(!is_local("vision") && !is_local(""));
        assert_eq!(resolve("local", "system"), "system");
        assert_eq!(resolve("local", "vision"), "enhanced");
        assert_eq!(resolve("api", "system"), "api");
    }

    #[test]
    fn recognized_lines_become_relative_blocks() {
        let lines =
            vec![OLine::new("Start", 10.0, 10.0, 40.0, 20.0), OLine::new("Game", 55.0, 11.0, 45.0, 19.0), OLine::new("Options", 10.0, 100.0, 60.0, 20.0)];
        let g = group_lines(&lines);
        let b = blocks_of(&g, &["开始游戏".to_string()], 200, 200);
        assert_eq!(b.len(), g.len());
        assert_eq!(b[0].text, "Start Game");
        assert_eq!(b[0].translation, "开始游戏");
        let r = b[0].rect.unwrap();
        assert!((r[0] - 0.05).abs() < 1e-6 && (r[1] - 0.05).abs() < 1e-6);
        assert!(b.iter().all(|x| x.rect.is_some_and(|r| r[0] + r[2] <= 1.0 + 1e-9 && r[1] + r[3] <= 1.0 + 1e-9)));
        assert_eq!(b.last().unwrap().translation, "", "没有译文的段落保留原文、译文为空");
    }

    #[test]
    fn data_urls_are_decoded() {
        let mut png = vec![];
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png));
        let (px, w, h) = decode_data_url(&url).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(&px[..4], &[30, 20, 10, 255], "BGRA");
        assert!(decode_data_url("data:image/png;base64,!!!").is_err());
    }
}
