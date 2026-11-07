//! 截图翻译：把图片交给支持看图的大模型，一次完成“认字 + 翻译 + 给出每段文字在图上的位置”。
//! 位置由模型估计（0–1000 的相对坐标），不保证像素级准确，所以界面上同时提供对照列表。

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 一张图的大小上限（data URL 的字符数）；HTTP 接口正文上限是 2 MiB。
pub const MAX_IMAGE: usize = 1_900_000;

pub fn check_image(image: &str) -> AppResult<()> {
    let ok_head = ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,"].iter().any(|h| image.starts_with(h));
    if !ok_head {
        return Err(AppError::invalid("只支持 PNG、JPEG、WebP 图片。"));
    }
    if image.len() > MAX_IMAGE {
        return Err(AppError::invalid("图片太大，请框选小一点的区域。"));
    }
    Ok(())
}

pub fn system(lang: &str) -> String {
    format!(
        "You are an OCR and translation engine. Find every piece of readable text in the image, grouping text that belongs together (a paragraph, a speech bubble, a button, a label, a table cell) into one block. \
         For each block give: \"text\" (the original text, line breaks as spaces), \"translation\" (a natural translation into {lang}; if the text is already in {lang} repeat it unchanged), \
         and \"box\": [x, y, w, h] — the block's bounding box relative to the whole image, each number 0–1000, origin at the top-left. \
         List blocks in reading order. Ignore decorative text you cannot read. Icons, pictograms and symbols (a person, a gear, a bell, a heart, arrows, checkboxes) are NOT text: never output them as blocks and never put them into a block's text. \
         Reply with ONE JSON object only, no commentary and no code fences: {{\"blocks\":[{{\"text\":\"...\",\"translation\":\"...\",\"box\":[0,0,100,50]}}]}}. \
         If the image has no text reply {{\"blocks\":[]}}."
    )
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    pub text: String,
    pub translation: String,
    /// 0–1 的相对位置 [x, y, w, h]；模型没给或给错就是 None（界面只在列表里显示）
    pub rect: Option<[f64; 4]>,
}

fn rect_of(v: Option<&Value>) -> Option<[f64; 4]> {
    let a = v?.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let n: Vec<f64> = a.iter().filter_map(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))).collect();
    if n.len() != 4 {
        return None;
    }
    // 有的模型直接给 0–1，有的给 0–1000
    let scale = if n.iter().all(|x| *x <= 1.0) { 1.0 } else { 1000.0 };
    let (x, y) = ((n[0] / scale).clamp(0.0, 1.0), (n[1] / scale).clamp(0.0, 1.0));
    let (w, h) = ((n[2] / scale).clamp(0.0, 1.0 - x), (n[3] / scale).clamp(0.0, 1.0 - y));
    (w > 0.003 && h > 0.003).then_some([x, y, w, h])
}

pub fn parse_blocks(resp: &str) -> Option<Vec<Block>> {
    let t = crate::ai::strip_fences(resp);
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    let v: Value = serde_json::from_str(&t[a..=b]).ok()?;
    let arr = v.get("blocks")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|b| {
                let text = b.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
                let translation = b.get("translation").and_then(Value::as_str).unwrap_or("").trim().to_string();
                (!text.is_empty() || !translation.is_empty()).then(|| Block { text, translation, rect: rect_of(b.get("box")) })
            })
            .take(200)
            .collect(),
    )
}

/// 视觉模型不可用时的提示（多数“模型不支持图片”的报错很难读懂）。
pub fn vision_hint(e: AppError) -> AppError {
    AppError::new(e.kind, format!("{}（识图翻译需要支持看图的模型，如 GPT-4o、Claude、Gemini、Qwen-VL、GLM-4V；到“设置 → AI”换一个。）", e.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blocks_and_normalizes_boxes() {
        let r = "```json\n{\"blocks\":[{\"text\":\"Hello\",\"translation\":\"你好\",\"box\":[100,200,300,50]},{\"text\":\"Bye\",\"translation\":\"再见\",\"box\":[0.1,0.2,0.3,0.1]},{\"text\":\"X\",\"translation\":\"叉\",\"box\":[1,2,3]},{\"text\":\"\",\"translation\":\"\"}]}\n```";
        let b = parse_blocks(r).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].rect, Some([0.1, 0.2, 0.3, 0.05]));
        assert_eq!(b[1].rect, Some([0.1, 0.2, 0.3, 0.1]));
        assert_eq!(b[2].rect, None);
    }

    #[test]
    fn boxes_are_clamped_inside_the_image() {
        let b = parse_blocks(r#"{"blocks":[{"text":"a","translation":"b","box":[900,900,500,500]}]}"#).unwrap();
        let r = b[0].rect.unwrap();
        assert!((r[0] + r[2]) <= 1.0 + 1e-9 && (r[1] + r[3]) <= 1.0 + 1e-9);
        assert!(parse_blocks("not json").is_none());
        assert_eq!(parse_blocks(r#"{"blocks":[]}"#).unwrap().len(), 0);
    }

    #[test]
    fn image_checks() {
        assert!(check_image("data:image/png;base64,AAAA").is_ok());
        assert!(check_image("http://x/y.png").is_err());
        assert!(check_image(&format!("data:image/jpeg;base64,{}", "A".repeat(MAX_IMAGE))).is_err());
    }
}
