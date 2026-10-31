//! 模型文件的登记、下载、导入、删除。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub const EVT_ML: &str = "ml://download";

/// 模型输入的归一化方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Norm {
    /// (x - 0.5) / 0.5，MODNet 这一类
    Half,
    /// ImageNet 的均值 / 方差，U²-Net 这一类
    ImageNet,
}

pub struct ModelDef {
    pub id: &'static str,
    pub label: &'static str,
    /// `matte`：人像抠图；`vocals`：人声分离；`denoise`：RNNoise 降噪；`whisper`：Whisper 语音识别
    pub kind: &'static str,
    /// 落盘的扩展名（不带点）
    pub ext: &'static str,
    pub url: &'static str,
    pub size_hint: &'static str,
    pub norm: Norm,
    /// 输入边长不是动态的模型会固定用这个边长（0 表示按画面比例自己定）
    pub fixed_side: usize,
    /// 人声分离模型的 FFT 点数（抠图模型填 0）
    pub n_fft: usize,
    pub note: &'static str,
}

/// 内置的模型登记。下载地址是公开的常见镜像，随时可能变动，所以界面里可以改；也可以直接导入自己下载的文件。
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "modnet",
        label: "MODNet（人像抠图，推荐）",
        kind: "matte",
        ext: "onnx",
        url: "https://huggingface.co/DavG25/modnet-pretrained-models/resolve/main/models/modnet_photographic_portrait_matting.onnx",
        size_hint: "约 25 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 0,
        note: "边缘细（头发丝），适合人像；输入输出都是任意尺寸的 ONNX",
    },
    ModelDef {
        id: "u2net_human",
        label: "U²-Net 人像分割（更轻）",
        kind: "matte",
        ext: "onnx",
        url: "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net_human_seg.onnx",
        size_hint: "约 170 MB",
        norm: Norm::ImageNet,
        fixed_side: 320,
        n_fft: 0,
        note: "分割为主，边缘比 MODNet 粗，但对全身 / 复杂姿势更稳",
    },
    ModelDef {
        id: "vocal_mdx",
        label: "MDX-Net 人声分离（Kim Vocal 2）",
        kind: "vocals",
        ext: "onnx",
        url: "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models/Kim_Vocal_2.onnx",
        size_hint: "约 60 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 7680,
        note: "把人声和伴奏分开；也可以导入同类的 MDX-Net ONNX 模型",
    },
    ModelDef {
        id: "rnnoise",
        label: "RNNoise 降噪模型（beguiling-drafter）",
        kind: "denoise",
        ext: "rnnn",
        url: "https://raw.githubusercontent.com/GregorR/rnnoise-models/master/beguiling-drafter-2018-08-30/bd.rnnn",
        size_hint: "约 0.3 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 0,
        note: "装上后，工具箱“音效”和剪辑里的降噪会用 RNNoise（ffmpeg 的 arnndn），人声更干净；没装时用 afftdn 频域降噪。需要 ffmpeg 带 arnndn 滤镜",
    },
    ModelDef {
        id: "whisper_tiny",
        label: "Whisper tiny（最快，准确度低）",
        kind: "whisper",
        ext: "bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        size_hint: "约 75 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 0,
        note: "本地转写（whisper.cpp 的 ggml 模型）。需要另外有 whisper-cli 程序，路径在“设置→AI”里填",
    },
    ModelDef {
        id: "whisper_base",
        label: "Whisper base（推荐入门）",
        kind: "whisper",
        ext: "bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        size_hint: "约 142 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 0,
        note: "速度和准确度比较平衡，中文也能用；视频摘要“用清影转写”默认够用",
    },
    ModelDef {
        id: "whisper_small",
        label: "Whisper small（更准，更慢）",
        kind: "whisper",
        ext: "bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        size_hint: "约 466 MB",
        norm: Norm::Half,
        fixed_side: 0,
        n_fft: 0,
        note: "中文、口音和嘈杂环境明显更准，转写时间约是 base 的 3 倍",
    },
];

pub fn def(id: &str) -> AppResult<&'static ModelDef> {
    MODELS.iter().find(|m| m.id == id).ok_or_else(|| AppError::invalid(format!("不认识的模型：{id}")))
}

pub fn dir(state: &AppState) -> PathBuf {
    state.data_dir.join("models")
}

pub fn path_of(data_dir: &Path, id: &str) -> PathBuf {
    let ext = MODELS.iter().find(|m| m.id == id).map_or("onnx", |m| m.ext);
    data_dir.join("models").join(format!("{id}.{ext}"))
}

/// 模型文件的路径，没装就报明白的错。
pub fn require(state: &AppState, id: &str) -> AppResult<PathBuf> {
    let d = def(id)?;
    let p = path_of(&state.data_dir, id);
    if p.is_file() {
        Ok(p)
    } else {
        Err(AppError::new(crate::error::ErrorKind::NeedUpdate, format!("还没有「{}」。请先在“设置 → 组件”里下载或导入。", d.label)))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub url: String,
    pub size_hint: String,
    pub note: String,
    pub installed: bool,
    pub size_bytes: u64,
}

pub fn status(data_dir: &Path) -> Vec<ModelStatus> {
    MODELS
        .iter()
        .map(|m| {
            let p = path_of(data_dir, m.id);
            let size = std::fs::metadata(&p).map(|x| x.len()).unwrap_or(0);
            ModelStatus {
                id: m.id.into(),
                label: m.label.into(),
                kind: m.kind.into(),
                url: m.url.into(),
                size_hint: m.size_hint.into(),
                note: m.note.into(),
                installed: p.is_file() && size > 0,
                size_bytes: size,
            }
        })
        .collect()
}

#[tauri::command]
pub fn ml_models(state: St<'_>) -> Vec<ModelStatus> {
    status(&state.data_dir)
}

/// 装好 Whisper 模型后，“设置→AI”里的模型路径还是空（或指向的文件没了）就自动填上。
fn autofill_whisper(state: &AppState, d: &ModelDef, target: &Path) {
    if d.kind != "whisper" {
        return;
    }
    let mut s = state.settings_raw();
    let cur = s.ai.whisper_model.trim();
    if !cur.is_empty() && Path::new(cur).is_file() {
        return;
    }
    s.ai.whisper_model = target.to_string_lossy().into_owned();
    if s.save(&state.settings_path).is_ok() {
        *state.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
    }
}

/// 模型文件的前几个字节没有固定魔数，只能挡住最明显的错误：太小、或者是网页（下载到了错误页）。
fn looks_like_onnx(head: &[u8], len: u64) -> Result<(), String> {
    if len < 1024 {
        return Err("文件太小，不像是模型".into());
    }
    let lower: Vec<u8> = head.iter().take(64).map(u8::to_ascii_lowercase).collect();
    if lower.windows(5).any(|w| w == b"<html") || lower.windows(9).any(|w| w == b"<!doctype") {
        return Err("下载到的是一个网页，不是模型文件。请检查地址，或手动下载后导入".into());
    }
    Ok(())
}

/// 下载（地址可以自己填）；先写到 `.part` 再改名，中途失败不会留下半个模型。`sha256` 不空就校验。
#[tauri::command]
pub async fn ml_model_download(app: AppHandle, state: St<'_>, id: String, url: Option<String>, sha256: Option<String>) -> AppResult<ModelStatus> {
    let d = def(&id)?;
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| d.url.to_string());
    let target = path_of(&state.data_dir, &id);
    if let Some(p) = target.parent() {
        std::fs::create_dir_all(p)?;
    }
    let part = target.with_extension(format!("{}.part", d.ext));
    let resp = state.dl_client.get(&url).send().await.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "模型文件"));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let (mut got, mut last, mut head): (u64, u64, Vec<u8>) = (0, 0, vec![]);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
        if head.len() < 64 {
            head.extend_from_slice(&chunk[..chunk.len().min(64)]);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        if got - last > 512 * 1024 {
            last = got;
            let _ = app.emit(EVT_ML, serde_json::json!({ "id": id, "got": got, "total": total }));
        }
    }
    file.flush().await?;
    drop(file);
    let fail = |msg: String| {
        let _ = std::fs::remove_file(&part);
        AppError::msg(msg)
    };
    if let Err(m) = looks_like_onnx(&head, got) {
        return Err(fail(m));
    }
    if let Some(want) = sha256.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
        let have = format!("{:x}", hasher.finalize());
        if have != want {
            return Err(fail(format!("校验不一致（应为 {want}，实际 {have}）。文件可能损坏或被替换。")));
        }
    }
    std::fs::rename(&part, &target)?;
    autofill_whisper(&state, d, &target);
    let _ = app.emit(EVT_ML, serde_json::json!({ "id": id, "got": got, "total": got }));
    status(&state.data_dir).into_iter().find(|s| s.id == id).ok_or_else(|| AppError::msg("模型登记丢失"))
}

/// 从本机导入一个已经下载好的模型文件。
#[tauri::command]
pub fn ml_model_import(state: St<'_>, id: String, path: String) -> AppResult<ModelStatus> {
    let d = def(&id)?;
    let src = PathBuf::from(&path);
    let meta = std::fs::metadata(&src).map_err(|_| AppError::not_found("找不到这个文件"))?;
    let mut head = vec![0u8; 64];
    let n = std::io::Read::read(&mut std::fs::File::open(&src)?, &mut head)?;
    head.truncate(n);
    looks_like_onnx(&head, meta.len()).map_err(AppError::invalid)?;
    let target = path_of(&state.data_dir, &id);
    if let Some(p) = target.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(&src, &target)?;
    autofill_whisper(&state, d, &target);
    status(&state.data_dir).into_iter().find(|s| s.id == id).ok_or_else(|| AppError::msg("模型登记丢失"))
}

#[tauri::command]
pub fn ml_model_remove(state: St<'_>, id: String) -> AppResult<()> {
    def(&id)?;
    let _ = std::fs::remove_file(path_of(&state.data_dir, &id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        let mut ids: Vec<_> = MODELS.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len(), "编号不重复");
        assert!(MODELS.iter().all(|m| m.url.starts_with("https://") && matches!(m.kind, "matte" | "vocals" | "denoise" | "whisper")));
        assert!(def("nope").is_err());
        assert_eq!(path_of(Path::new("/d"), "modnet"), PathBuf::from("/d/models/modnet.onnx"));
        assert_eq!(path_of(Path::new("/d"), "rnnoise"), crate::audiofx::rnn_model_path(Path::new("/d")), "RNNoise 要落在降噪代码找的位置");
        assert_eq!(path_of(Path::new("/d"), "whisper_base"), PathBuf::from("/d/models/whisper_base.bin"));
    }

    #[test]
    fn html_error_pages_and_tiny_files_are_rejected() {
        assert!(looks_like_onnx(b"<!DOCTYPE html><html>", 5000).is_err());
        assert!(looks_like_onnx(b"<html>", 5000).is_err());
        assert!(looks_like_onnx(b"\x08\x07\x12", 100).is_err());
        assert!(looks_like_onnx(b"\x08\x07\x12\x07pytorch", 5_000_000).is_ok());
    }

    #[test]
    fn status_reflects_the_files_on_disk() {
        let d = std::env::temp_dir().join(format!("cc-ml-{}", std::process::id()));
        std::fs::create_dir_all(d.join("models")).unwrap();
        assert!(status(&d).iter().all(|s| !s.installed));
        std::fs::write(path_of(&d, "modnet"), vec![1u8; 2048]).unwrap();
        let s = status(&d);
        assert!(s.iter().find(|s| s.id == "modnet").unwrap().installed);
        assert!(!s.iter().find(|s| s.id == "vocal_mdx").unwrap().installed);
        let _ = std::fs::remove_dir_all(d);
    }
}
