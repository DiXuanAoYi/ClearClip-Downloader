//! whisper.cpp 的命令行程序（whisper-cli）：在“设置 → 组件”里一键下载。
//!
//! 官方只在 Windows 上提供现成的压缩包（`whisper-bin-x64.zip`，里面是 `Release/whisper-cli.exe` 和它的 DLL）；
//! macOS / Linux 没有，需要 `brew install whisper-cpp` 或自己编译（装好后会自动在 PATH 里找到）。
//! 下载地址：先问 GitHub 的发布列表，取最新的、带这个压缩包的一版；问不到就用写死的一个版本。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

const ASSET: &str = "whisper-bin-x64.zip";
const RELEASES_API: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=30";
const FALLBACK_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-bin-x64.zip";
/// 进度事件沿用模型下载的通道，用这个 id 区分。
pub const ID: &str = "whisper-cli";

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "whisper-cli.exe"
    } else {
        "whisper-cli"
    }
}

pub fn dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("whisper-cpp")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperCliStatus {
    pub installed: bool,
    pub path: Option<String>,
    /// 这个系统有官方现成的包吗（目前只有 Windows）
    pub auto_install: bool,
    pub note: String,
}

fn status_of(state: &AppState) -> WhisperCliStatus {
    let managed = dir_of(&state.data_dir).join(bin_name());
    let settings = state.settings();
    let found = crate::ai::find_whisper_bin(&settings).or_else(|| managed.is_file().then(|| managed.clone()));
    let auto = cfg!(all(windows, any(target_arch = "x86_64", target_arch = "aarch64")));
    let note = if found.is_some() {
        String::new()
    } else if auto {
        "官方 whisper.cpp 的 Windows 版（约 10–20 MB），不含模型。".to_string()
    } else if cfg!(target_os = "macos") {
        "macOS 官方没有现成的包：在终端运行 brew install whisper-cpp，装好后点“重新检测”。".to_string()
    } else {
        "Linux 官方没有现成的包：请用发行版的软件包或按 whisper.cpp 的说明编译，把 whisper-cli 放进 PATH 后点“重新检测”。".to_string()
    };
    WhisperCliStatus { installed: found.is_some(), path: found.map(|p| p.to_string_lossy().into_owned()), auto_install: auto, note }
}

#[tauri::command]
pub fn whisper_cli_status(state: St<'_>) -> WhisperCliStatus {
    status_of(&state)
}

/// 装好后、“设置 → AI”里的程序路径还是空（或文件没了）就自动填上。
fn autofill(state: &AppState, bin: &Path) {
    let mut s = state.settings_raw();
    let cur = s.ai.whisper_bin.trim();
    if !cur.is_empty() && Path::new(cur).is_file() {
        return;
    }
    s.ai.whisper_bin = bin.to_string_lossy().into_owned();
    if s.save(&state.settings_path).is_ok() {
        *state.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
    }
}

/// 从发布列表（GitHub API 返回的 JSON）里找最新一个带 `whisper-bin-x64.zip` 的下载地址。
pub fn pick_asset_url(releases: &serde_json::Value) -> Option<String> {
    releases.as_array()?.iter().filter(|r| !r["draft"].as_bool().unwrap_or(false)).find_map(|r| {
        r["assets"].as_array()?.iter().find(|a| a["name"].as_str() == Some(ASSET)).and_then(|a| a["browser_download_url"].as_str()).map(str::to_string)
    })
}

/// 把压缩包里所有文件摊平放进 `dest`（exe 和 DLL 要在同一个目录），返回 whisper-cli 的路径。
pub fn extract_flat(zip_path: &Path, dest: &Path) -> AppResult<PathBuf> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?).map_err(|e| AppError::msg(format!("压缩包打不开：{e}")))?;
    std::fs::create_dir_all(dest)?;
    let mut bin = None;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| AppError::msg(format!("压缩包损坏：{e}")))?;
        if f.is_dir() {
            continue;
        }
        // 只取文件名，丢掉目录部分：既摊平了 Release/，也挡住 ../ 之类的路径
        let Some(name) = f.name().rsplit(['/', '\\']).next().map(str::to_string).filter(|n| !n.is_empty() && n != "..") else { continue };
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".exe") || lower.ends_with(".dll") || lower.ends_with(".txt") || lower == "whisper-cli") {
            continue;
        }
        let out = dest.join(&name);
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
        if lower == "whisper-cli.exe" || lower == "whisper-cli" {
            bin = Some(out);
        }
    }
    bin.ok_or_else(|| AppError::msg("压缩包里没有 whisper-cli。可能官方改了打包方式，请到 whisper.cpp 的发布页手动下载后，在“设置 → AI”里指定位置。"))
}

/// 能不能启动：缺少运行库时 Windows 会直接退出且没有输出。
async fn smoke_test(bin: &Path) -> AppResult<()> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--help").stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
        .await
        .map_err(|_| AppError::msg("whisper-cli 启动超时"))?
        .map_err(|e| AppError::msg(format!("无法运行 whisper-cli：{e}")))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).to_lowercase();
    if text.contains("usage") || text.contains("whisper") {
        Ok(())
    } else {
        Err(AppError::msg("whisper-cli 启动后没有输出，可能缺少 Microsoft Visual C++ 运行库（VC++ Redistributable），安装后重试。"))
    }
}

#[tauri::command]
pub async fn whisper_cli_install(app: AppHandle, state: St<'_>) -> AppResult<WhisperCliStatus> {
    if !status_of(&state).auto_install {
        return Err(AppError::invalid("这个系统没有官方现成的包，请按提示自行安装后点“重新检测”。"));
    }
    let settings = state.settings();
    let mirrors = settings.component_mirrors.clone();
    let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": 0, "total": 0 }));
    // 1) 找下载地址
    let api = crate::tools::candidates(RELEASES_API, &[]);
    let mut url = None;
    for u in api {
        if let Ok(r) = state.dl_client.get(&u).header("Accept", "application/vnd.github+json").header("User-Agent", "ClearClip").send().await {
            if r.status().is_success() {
                if let Ok(j) = r.json::<serde_json::Value>().await {
                    url = pick_asset_url(&j);
                    if url.is_some() {
                        break;
                    }
                }
            }
        }
    }
    let url = url.unwrap_or_else(|| FALLBACK_URL.to_string());
    // 2) 下载（镜像前缀在前，官方地址最后）
    let tmp = state.data_dir.join("whisper-cpp.zip.part");
    let mut last_err = AppError::msg("下载失败");
    let mut done = false;
    for u in crate::tools::candidates(&url, &mirrors) {
        match download(&app, &state, &u, &tmp).await {
            Ok(()) => {
                done = true;
                break;
            }
            Err(e) => last_err = e,
        }
    }
    if !done {
        let _ = std::fs::remove_file(&tmp);
        return Err(last_err.context("下载 whisper-cli 失败："));
    }
    // 3) 解压到专用目录，校验能启动
    let dest = dir_of(&state.data_dir);
    let tmp2 = tmp.clone();
    let dest2 = dest.clone();
    let bin = tokio::task::spawn_blocking(move || extract_flat(&tmp2, &dest2)).await.map_err(|e| AppError::msg(e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    let bin = bin?;
    if let Err(e) = smoke_test(&bin).await {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    autofill(&state, &bin);
    let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": 1, "total": 1 }));
    Ok(status_of(&state))
}

async fn download(app: &AppHandle, state: &AppState, url: &str, part: &Path) -> AppResult<()> {
    let resp = state.dl_client.get(url).send().await.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "whisper-cli"));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(part).await?;
    let (mut got, mut last) = (0u64, 0u64);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        if got - last > 256 * 1024 {
            last = got;
            let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": got, "total": total }));
        }
    }
    file.flush().await?;
    if got < 100_000 {
        return Err(AppError::msg("下载到的文件太小，不是压缩包"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn picks_the_newest_release_that_has_the_windows_zip() {
        let j = serde_json::json!([
            {"draft": false, "assets": [{"name": "source.zip", "browser_download_url": "https://x/s"}]},
            {"draft": true, "assets": [{"name": "whisper-bin-x64.zip", "browser_download_url": "https://x/draft"}]},
            {"draft": false, "assets": [{"name": "whisper-bin-x64.zip", "browser_download_url": "https://x/ok"}, {"name": "whisper-bin-Win32.zip", "browser_download_url": "https://x/32"}]},
            {"draft": false, "assets": [{"name": "whisper-bin-x64.zip", "browser_download_url": "https://x/old"}]}
        ]);
        assert_eq!(pick_asset_url(&j).as_deref(), Some("https://x/ok"));
        assert_eq!(pick_asset_url(&serde_json::json!([])), None);
        assert_eq!(pick_asset_url(&serde_json::json!({"message": "rate limited"})), None);
    }

    #[test]
    fn extracts_exe_and_dlls_flat_and_ignores_path_tricks() {
        let dir = std::env::temp_dir().join(format!("cc-whisper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("w.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for (n, body) in [("Release/whisper-cli.exe", "exe"), ("Release/ggml.dll", "dll"), ("Release/notes.md", "x"), ("../evil.dll", "bad")] {
                z.start_file(n, o).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let out = dir.join("out");
        let bin = extract_flat(&zip_path, &out).unwrap();
        assert_eq!(bin, out.join("whisper-cli.exe"));
        assert!(out.join("ggml.dll").is_file());
        assert!(!out.join("notes.md").exists());
        assert!(!dir.join("evil.dll").exists(), "../ 不能跳出目录");
        assert!(out.join("evil.dll").is_file(), "只取文件名，落在目标目录里");
        // 没有 whisper-cli 的包要报错
        let empty = dir.join("e.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&empty).unwrap());
            z.start_file("a.dll", zip::write::SimpleFileOptions::default()).unwrap();
            z.finish().unwrap();
        }
        assert!(extract_flat(&empty, &dir.join("o2")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
