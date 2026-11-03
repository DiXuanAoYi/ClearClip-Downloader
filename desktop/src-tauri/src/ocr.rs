//! 识字调度：Windows 自带识字 / 增强识字组件 / 云端识字服务（OCR.space、百度、Google）。
//! 视觉模型识字在 `livetr` 里单独走（它一次完成识字 + 翻译）。

use std::path::Path;

use base64::Engine as _;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::livecore::{self, OLine};
use crate::settings::{LiveSettings, OcrApi};

fn f(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

// ---------- 语言代码 ----------

/// 目标 / 来源语言的粗略类别 → OCR.space 的语言代码。
pub fn ocrspace_lang(tag: &str) -> &'static str {
    let t = tag.to_lowercase();
    match t.split(['-', '_']).next().unwrap_or("") {
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => {
            if t.contains("hant") || t.contains("tw") || t.contains("hk") {
                "cht"
            } else {
                "chs"
            }
        }
        "ru" => "rus",
        "de" => "ger",
        "fr" => "fre",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "ar" => "ara",
        "th" => "tha",
        "vi" => "vnm",
        "en" => "eng",
        _ => "eng",
    }
}

pub fn baidu_lang(tag: &str) -> &'static str {
    let t = tag.to_lowercase();
    match t.split(['-', '_']).next().unwrap_or("") {
        "ja" => "JAP",
        "ko" => "KOR",
        "zh" => "CHN_ENG",
        "ru" => "RUS",
        "de" => "GER",
        "fr" => "FRE",
        "es" => "SPA",
        "pt" => "POR",
        "it" => "ITA",
        "en" => "ENG",
        _ => "auto_detect",
    }
}

// ---------- 解析各家返回 ----------

/// OCR.space：`ParsedResults[].TextOverlay.Lines[]`。
pub fn parse_ocrspace(v: &Value) -> AppResult<Vec<OLine>> {
    if v.get("IsErroredOnProcessing").and_then(Value::as_bool).unwrap_or(false) {
        let m = v
            .get("ErrorMessage")
            .and_then(|m| m.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("；")).or_else(|| m.as_str().map(str::to_string)))
            .unwrap_or_default();
        return Err(AppError::msg(format!("OCR.space：{}", if m.is_empty() { "识别失败".to_string() } else { m })));
    }
    let mut out = vec![];
    for r in v.get("ParsedResults").and_then(Value::as_array).into_iter().flatten() {
        for l in r.pointer("/TextOverlay/Lines").and_then(Value::as_array).into_iter().flatten() {
            let words: Vec<&Value> = l.get("Words").and_then(Value::as_array).into_iter().flatten().collect();
            let text = l.get("LineText").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if text.is_empty() || words.is_empty() {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for w in words {
                let (left, top, ww, hh) =
                    (f(&w["Left"]).unwrap_or(0.0), f(&w["Top"]).unwrap_or(0.0), f(&w["Width"]).unwrap_or(0.0), f(&w["Height"]).unwrap_or(0.0));
                x0 = x0.min(left);
                y0 = y0.min(top);
                x1 = x1.max(left + ww);
                y1 = y1.max(top + hh);
            }
            if x1 > x0 && y1 > y0 {
                out.push(OLine::new(&text, x0, y0, x1 - x0, y1 - y0));
            }
        }
    }
    Ok(out)
}

/// 百度高精度含位置版：`words_result[].{words, location{left,top,width,height}}`。
pub fn parse_baidu(v: &Value) -> AppResult<Vec<OLine>> {
    if let Some(code) = v.get("error_code") {
        let msg = v.get("error_msg").and_then(Value::as_str).unwrap_or("");
        return Err(AppError::msg(format!("百度识字出错（{code}）：{msg}")));
    }
    Ok(v.get("words_result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let text = w.get("words").and_then(Value::as_str)?.trim();
            let l = w.get("location")?;
            let (x, y, ww, hh) = (f(&l["left"])?, f(&l["top"])?, f(&l["width"])?, f(&l["height"])?);
            (!text.is_empty() && ww > 0.0 && hh > 0.0).then(|| OLine::new(text, x, y, ww, hh))
        })
        .collect())
}

/// Google Cloud Vision：`responses[0].textAnnotations[1..]` 是一个个词，坐标在 `boundingPoly.vertices`。
pub fn parse_google(v: &Value) -> AppResult<Vec<OLine>> {
    let r = v.pointer("/responses/0").ok_or_else(|| AppError::msg("Google 识字没有返回结果"))?;
    if let Some(e) = r.get("error") {
        return Err(AppError::msg(format!("Google 识字出错：{}", e.get("message").and_then(Value::as_str).unwrap_or("未知错误"))));
    }
    let anns = r.get("textAnnotations").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(anns
        .iter()
        .skip(1)
        .filter_map(|a| {
            let text = a.get("description").and_then(Value::as_str)?.trim();
            let vs = a.pointer("/boundingPoly/vertices")?.as_array()?;
            let xs: Vec<f32> = vs.iter().map(|p| f(&p["x"]).unwrap_or(0.0)).collect();
            let ys: Vec<f32> = vs.iter().map(|p| f(&p["y"]).unwrap_or(0.0)).collect();
            let (x0, x1) = (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(0.0, f32::max));
            let (y0, y1) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(0.0, f32::max));
            (!text.is_empty() && x1 > x0 && y1 > y0).then(|| OLine::new(text, x0, y0, x1 - x0, y1 - y0))
        })
        .collect())
}

// ---------- 调用云端 ----------

pub fn png_data(bgra: &[u8], w: usize, h: usize) -> AppResult<Vec<u8>> {
    crate::scrcap::encode_png(&crate::scrcap::bgra_to_rgb(bgra), w as u32, h as u32).map_err(AppError::msg)
}

fn net_err(e: reqwest::Error) -> AppError {
    AppError::classify(crate::error::describe_reqwest(&e))
}

async fn json_of(r: reqwest::Response, who: &str) -> AppResult<Value> {
    let status = r.status();
    let text = r.text().await.map_err(net_err)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Ok(v),
        Err(_) if !status.is_success() => Err(AppError::from_status(status.as_u16(), who)),
        Err(_) => Err(AppError::msg(format!("{who}返回的内容不是 JSON"))),
    }
}

pub async fn api_recognize(client: &reqwest::Client, api: &OcrApi, lang: &str, png: &[u8]) -> AppResult<Vec<OLine>> {
    if api.key.is_empty() {
        return Err(AppError::invalid("还没有填写识字接口的密钥。到“屏幕翻译 → 识字”里填写。"));
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    match api.provider.as_str() {
        "baidu" => {
            if api.secret.is_empty() {
                return Err(AppError::invalid("百度识字需要同时填写 API Key 和 Secret Key。"));
            }
            let base = if api.endpoint.is_empty() { "https://aip.baidubce.com" } else { api.endpoint.trim_end_matches('/') };
            let tok = client
                .get(format!("{base}/oauth/2.0/token"))
                .query(&[("grant_type", "client_credentials"), ("client_id", api.key.as_str()), ("client_secret", api.secret.as_str())])
                .send()
                .await
                .map_err(net_err)?;
            let tok = json_of(tok, "百度").await?;
            let token = tok.get("access_token").and_then(Value::as_str).ok_or_else(|| {
                AppError::msg(format!("百度没有给出 access_token：{}", tok.get("error_description").and_then(Value::as_str).unwrap_or("请检查 Key")))
            })?;
            let r = client
                .post(format!("{base}/rest/2.0/ocr/v1/accurate"))
                .query(&[("access_token", token)])
                .form(&[("image", b64.as_str()), ("language_type", baidu_lang(lang)), ("vertexes_location", "false")])
                .send()
                .await
                .map_err(net_err)?;
            parse_baidu(&json_of(r, "百度").await?)
        }
        "google" => {
            let base = if api.endpoint.is_empty() { "https://vision.googleapis.com" } else { api.endpoint.trim_end_matches('/') };
            let body = serde_json::json!({"requests":[{"image":{"content": b64},"features":[{"type":"TEXT_DETECTION"}]}]});
            let r = client.post(format!("{base}/v1/images:annotate")).query(&[("key", api.key.as_str())]).json(&body).send().await.map_err(net_err)?;
            parse_google(&json_of(r, "Google").await?)
        }
        _ => {
            let url = if api.endpoint.is_empty() { "https://api.ocr.space/parse/image".to_string() } else { api.endpoint.clone() };
            let data = format!("data:image/png;base64,{b64}");
            let r = client
                .post(url)
                .header("apikey", api.key.as_str())
                .form(&[
                    ("base64Image", data.as_str()),
                    ("language", ocrspace_lang(lang)),
                    ("isOverlayRequired", "true"),
                    ("OCREngine", "2"),
                    ("scale", "true"),
                ])
                .send()
                .await
                .map_err(net_err)?;
            parse_ocrspace(&json_of(r, "OCR.space").await?)
        }
    }
}

// ---------- 调度 ----------

pub struct Ctx<'a> {
    pub data_dir: &'a Path,
    pub client: &'a reqwest::Client,
    pub live: &'a LiveSettings,
}

/// 识字用的语言：设置里选的，否则用“来源语言猜测”（空 = 自动）。
fn lang_hint(live: &LiveSettings) -> String {
    live.ocr_lang.trim().to_string()
}

/// Windows 自带识字对小字不友好：画面小就先放大。返回的坐标已换回原尺寸。
fn system_ocr(bgra: &[u8], w: usize, h: usize, lang: &str) -> AppResult<Vec<OLine>> {
    let k = if w.max(h) < 1100 { 2 } else { 1 };
    let (lines, _) = if k == 1 {
        crate::livewin::ocr(bgra, w, h, lang).map_err(AppError::msg)?
    } else {
        let (up, uw, uh) = livecore::upscale(bgra, w, h, k);
        crate::livewin::ocr(&up, uw, uh, lang).map_err(AppError::msg)?
    };
    let s = k as f32;
    Ok(lines.into_iter().map(|l| OLine::new(&l.text, l.x / s, l.y / s, l.w / s, l.h / s)).collect())
}

/// 识别一帧。`engine` 是 system / enhanced / api。
pub fn recognize(cx: &Ctx<'_>, engine: &str, bgra: &[u8], w: usize, h: usize) -> AppResult<Vec<OLine>> {
    match engine {
        "enhanced" => crate::ocr_paddle::recognize(cx.data_dir, bgra, w, h),
        "api" => {
            let png = png_data(bgra, w, h)?;
            let lang = lang_hint(cx.live);
            tauri::async_runtime::block_on(api_recognize(cx.client, &cx.live.ocr_api, &lang, &png))
        }
        _ => {
            if !crate::livewin::SUPPORTED {
                return Err(AppError::msg("Windows 自带识字只在 Windows 上可用。请换成增强识字或识字接口。"));
            }
            system_ocr(bgra, w, h, &lang_hint(cx.live)).map_err(|e| {
                if e.message.contains("语言包") {
                    AppError::new(crate::error::ErrorKind::NeedUpdate, e.message)
                } else {
                    e
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn language_codes() {
        assert_eq!(ocrspace_lang("ja-JP"), "jpn");
        assert_eq!(ocrspace_lang("zh-Hant-TW"), "cht");
        assert_eq!(ocrspace_lang("zh-CN"), "chs");
        assert_eq!(ocrspace_lang(""), "eng");
        assert_eq!(baidu_lang("ko"), "KOR");
        assert_eq!(baidu_lang("xx"), "auto_detect");
    }

    #[test]
    fn parses_ocrspace_lines() {
        let v = json!({"IsErroredOnProcessing": false, "ParsedResults":[{"TextOverlay":{"Lines":[
            {"LineText":"Start Game","MaxHeight":20,"MinTop":10,"Words":[
                {"WordText":"Start","Left":10,"Top":10,"Height":20,"Width":40},
                {"WordText":"Game","Left":55,"Top":11,"Height":19,"Width":45}]},
            {"LineText":"","Words":[]}]}}]});
        let l = parse_ocrspace(&v).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].x, l[0].y, l[0].w, l[0].h), (10.0, 10.0, 90.0, 20.0));
        let e = json!({"IsErroredOnProcessing": true, "ErrorMessage": ["File size exceeds"]});
        assert!(parse_ocrspace(&e).unwrap_err().message.contains("File size"));
    }

    #[test]
    fn parses_baidu_lines() {
        let v = json!({"words_result":[{"words":"设置","location":{"left":5,"top":6,"width":40,"height":18}},{"words":"","location":{"left":0,"top":0,"width":1,"height":1}}]});
        let l = parse_baidu(&v).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "设置");
        assert!(parse_baidu(&json!({"error_code": 110, "error_msg": "Access token invalid"})).is_err());
    }

    #[test]
    fn parses_google_words() {
        let v = json!({"responses":[{"textAnnotations":[
            {"description":"Hello World","boundingPoly":{"vertices":[]}},
            {"description":"Hello","boundingPoly":{"vertices":[{"x":10,"y":10},{"x":50,"y":10},{"x":50,"y":30},{"x":10,"y":30}]}},
            {"description":"World","boundingPoly":{"vertices":[{"x":56,"y":11},{"x":100,"y":11},{"x":100,"y":31},{"x":56,"y":31}]}}]}]});
        let l = parse_google(&v).unwrap();
        assert_eq!(l.len(), 2);
        // 同一行的词由 group_lines 接成一句
        let g = livecore::group_lines(&l);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].text, "Hello World");
        assert!(parse_google(&json!({"responses":[{"error":{"message":"API key not valid"}}]})).unwrap_err().message.contains("API key"));
    }
}
