//! 外部组件管理（yt-dlp、ffmpeg）：查找、首次使用时下载、SHA-256 校验、更新与回退、手动导入。
//!
//! 下载源为官方 GitHub Releases（yt-dlp 官方构建、BtbN 的 LGPL 版 ffmpeg），可在设置里添加 GitHub 镜像前缀；
//! 校验值取自同一发布里的官方校验文件，不一致时丢弃下载的文件。

use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::http::{self as http_engine, HttpOptions, HttpRequest};
use crate::engine::{ResumeMeta, CTRL_RUN};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::AppState;

pub const EVT_PROGRESS: &str = "tools://progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
}

impl Tool {
    pub fn all() -> [Tool; 2] {
        [Tool::YtDlp, Tool::Ffmpeg]
    }

    pub fn id(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
        }
    }

    pub fn parse(id: &str) -> Option<Tool> {
        Tool::all().into_iter().find(|t| t.id() == id)
    }

    pub fn bin_name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Tool::YtDlp, true) => "yt-dlp.exe",
            (Tool::YtDlp, false) => "yt-dlp",
            (Tool::Ffmpeg, true) => "ffmpeg.exe",
            (Tool::Ffmpeg, false) => "ffmpeg",
        }
    }

    fn version_arg(self) -> &'static str {
        match self {
            Tool::YtDlp => "--version",
            Tool::Ffmpeg => "-version",
        }
    }
}

/// 组件管理的运行状态：正在安装的组件、缓存的 yt-dlp 支持站点列表。
#[derive(Default)]
pub struct ToolsState {
    busy: Mutex<HashSet<Tool>>,
    pub extractors: Mutex<Option<Vec<String>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: &'static str,
    pub installed: bool,
    /// 位于程序管理的组件目录（可更新、可回退）
    pub managed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub has_previous: bool,
    /// 当前系统是否支持自动下载
    pub auto_install: bool,
    pub note: Option<String>,
    pub busy: bool,
    /// ffmpeg 的版本：full（完整版，GPL）/ lite（精简版，LGPL）。其他组件为 None
    pub edition: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    pub tool: &'static str,
    /// prepare / download / verify / extract / done
    pub stage: &'static str,
    pub received: u64,
    pub total: Option<u64>,
}

/// 组件位置：优先使用程序管理的组件目录，其次系统 PATH。
pub fn resolve(st: &AppState, tool: Tool) -> Option<PathBuf> {
    let managed = st.tools_dir.join(tool.bin_name());
    if managed.is_file() {
        return Some(managed);
    }
    find_in_path(tool.bin_name())
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).chain(extra_dirs()).map(|d| d.join(name)).find(|p| is_executable(p))
}

/// 从桌面启动的程序拿到的 PATH 往往很短（macOS 的 Finder 启动时没有 Homebrew 的目录），这里补上常见的安装位置。
fn extra_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut v: Vec<PathBuf> = vec![];
    match std::env::consts::OS {
        "macos" => v.extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].map(PathBuf::from)),
        "linux" => v.extend(["/usr/local/bin", "/snap/bin"].map(PathBuf::from)),
        "windows" => {
            if let Some(h) = &home {
                v.push(h.join("scoop").join("shims"));
            }
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                v.push(PathBuf::from(l).join("Microsoft").join("WinGet").join("Links"));
            }
            v.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
        }
        _ => {}
    }
    v
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    };
    let arch = if std::env::consts::ARCH == "aarch64" { "aarch64" } else { "x86_64" };
    format!("{os}-{arch}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    None,
    Zip,
    TarXz,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub file_url: String,
    pub sums_url: String,
    /// 校验文件里对应的文件名
    pub asset: String,
    pub archive: Archive,
}

const YTDLP_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
const FFMPEG_BASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/";

/// 各平台的官方下载源。`edition`：ffmpeg 的版本，`lite` 精简版（LGPL，体积小，没有 x264）/ `full` 完整版（GPL，带 x264、x265、vidstab、zimg 等）。
/// 完整版只在用户主动选择后下载，不随程序分发。macOS 没有官方构建，请用 Homebrew 安装（brew install ffmpeg 就是完整版）或手动导入。
pub fn source_for(tool: Tool, platform: &str, edition: &str) -> Option<Source> {
    let flavor = if edition == "full" { "gpl" } else { "lgpl" };
    let (base, sums, asset, archive): (&str, &str, String, Archive) = match (tool, platform) {
        (Tool::YtDlp, "windows-x86_64") => (YTDLP_BASE, "SHA2-256SUMS", "yt-dlp.exe".into(), Archive::None),
        (Tool::YtDlp, "windows-aarch64") => (YTDLP_BASE, "SHA2-256SUMS", "yt-dlp_arm64.exe".into(), Archive::None),
        (Tool::YtDlp, p) if p.starts_with("macos") => (YTDLP_BASE, "SHA2-256SUMS", "yt-dlp_macos".into(), Archive::None),
        (Tool::YtDlp, "linux-x86_64") => (YTDLP_BASE, "SHA2-256SUMS", "yt-dlp_linux".into(), Archive::None),
        (Tool::YtDlp, "linux-aarch64") => (YTDLP_BASE, "SHA2-256SUMS", "yt-dlp_linux_aarch64".into(), Archive::None),
        (Tool::Ffmpeg, "windows-x86_64") => (FFMPEG_BASE, "checksums.sha256", format!("ffmpeg-master-latest-win64-{flavor}.zip"), Archive::Zip),
        (Tool::Ffmpeg, "windows-aarch64") => (FFMPEG_BASE, "checksums.sha256", format!("ffmpeg-master-latest-winarm64-{flavor}.zip"), Archive::Zip),
        (Tool::Ffmpeg, "linux-x86_64") => (FFMPEG_BASE, "checksums.sha256", format!("ffmpeg-master-latest-linux64-{flavor}.tar.xz"), Archive::TarXz),
        (Tool::Ffmpeg, "linux-aarch64") => (FFMPEG_BASE, "checksums.sha256", format!("ffmpeg-master-latest-linuxarm64-{flavor}.tar.xz"), Archive::TarXz),
        _ => return None,
    };
    Some(Source { file_url: format!("{base}{asset}"), sums_url: format!("{base}{sums}"), asset, archive })
}

/// 镜像前缀 + 原地址，镜像在前、官方地址在最后。
pub fn candidates(url: &str, mirrors: &[String]) -> Vec<String> {
    mirrors.iter().map(|m| format!("{m}{url}")).chain(std::iter::once(url.to_string())).collect()
}

/// 解析 `<sha256>  <文件名>` 格式的校验文件。
pub fn parse_sums(text: &str, asset: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let name = it.next()?.trim_start_matches('*');
        (name == asset && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = BufReader::new(std::fs::File::open(path)?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 从压缩包里取出指定文件名的可执行文件。
pub fn extract_binary(archive: &Path, kind: Archive, bin: &str, out: &Path) -> AppResult<()> {
    match kind {
        Archive::None => {
            std::fs::copy(archive, out)?;
        }
        Archive::Zip => {
            let f = std::fs::File::open(archive)?;
            let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::msg(format!("压缩包损坏：{e}")))?;
            let idx = (0..zip.len())
                .find(|&i| zip.by_index(i).ok().and_then(|e| e.enclosed_name()).and_then(|p| p.file_name().map(|n| n == bin)).unwrap_or(false))
                .ok_or_else(|| AppError::msg(format!("压缩包里没有找到 {bin}")))?;
            let mut entry = zip.by_index(idx).map_err(|e| AppError::msg(e.to_string()))?;
            let mut w = std::fs::File::create(out)?;
            std::io::copy(&mut entry, &mut w)?;
        }
        Archive::TarXz => {
            // 先解压成临时 tar 文件，避免把整个压缩包读进内存
            let tar_path = archive.with_extension("tar.tmp");
            {
                let mut input = BufReader::new(std::fs::File::open(archive)?);
                let mut output = std::io::BufWriter::new(std::fs::File::create(&tar_path)?);
                lzma_rs::xz_decompress(&mut input, &mut output).map_err(|e| AppError::msg(format!("解压失败：{e}")))?;
                output.flush()?;
            }
            let result = (|| -> AppResult<()> {
                let mut ar = tar::Archive::new(std::fs::File::open(&tar_path)?);
                for entry in ar.entries()? {
                    let mut entry = entry?;
                    let is_bin = entry.path().ok().and_then(|p| p.file_name().map(|n| n == bin)).unwrap_or(false);
                    if is_bin && entry.header().entry_type().is_file() {
                        let mut w = std::fs::File::create(out)?;
                        std::io::copy(&mut entry, &mut w)?;
                        return Ok(());
                    }
                }
                Err(AppError::msg(format!("压缩包里没有找到 {bin}")))
            })();
            let _ = std::fs::remove_file(&tar_path);
            result?;
        }
    }
    make_executable(out);
    Ok(())
}

fn make_executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = p;
}

/// 运行组件取版本号（第一行）。
pub async fn version_of(tool: Tool, path: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg(tool.version_arg()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_string();
    Some(parse_version(tool, &first)).filter(|v| !v.is_empty())
}

/// ffmpeg 是不是完整版（编译时启用了 GPL，带 x264 等）。
pub async fn ffmpeg_is_gpl(path: &Path) -> bool {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg("-version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
        Ok(Ok(out)) => {
            String::from_utf8_lossy(&out.stdout).lines().any(|l| l.starts_with("configuration:") && l.split_whitespace().any(|w| w == "--enable-gpl"))
        }
        _ => false,
    }
}

pub fn parse_version(tool: Tool, first_line: &str) -> String {
    match tool {
        Tool::YtDlp => first_line.to_string(),
        Tool::Ffmpeg => first_line.strip_prefix("ffmpeg version ").and_then(|r| r.split_whitespace().next()).unwrap_or(first_line).to_string(),
    }
}

pub async fn status(st: &AppState, tool: Tool) -> ToolStatus {
    let managed_path = st.tools_dir.join(tool.bin_name());
    let path = resolve(st, tool);
    let version = match &path {
        Some(p) => version_of(tool, p).await,
        None => None,
    };
    let auto = source_for(tool, &platform_key(), "lite").is_some();
    let note = match (tool, auto, path.is_some()) {
        (Tool::Ffmpeg, false, false) => {
            Some("macOS 请用 Homebrew 安装（brew install ffmpeg，是带 x264 的完整版），装好后重新打开清影；也可以下载后手动导入。".into())
        }
        (_, _, true) if version.is_none() => Some("找到了程序但无法运行，请重新安装或导入。".into()),
        _ => None,
    };
    let edition = match (&path, tool) {
        (Some(p), Tool::Ffmpeg) if version.is_some() => Some(if ffmpeg_is_gpl(p).await { "full" } else { "lite" }),
        _ => None,
    };
    ToolStatus {
        id: tool.id(),
        edition,
        installed: path.is_some() && version.is_some(),
        managed: managed_path.is_file(),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        version,
        has_previous: prev_path(st, tool).is_file(),
        auto_install: auto,
        note,
        busy: st.tools.busy.lock().unwrap_or_else(|e| e.into_inner()).contains(&tool),
    }
}

fn prev_path(st: &AppState, tool: Tool) -> PathBuf {
    st.tools_dir.join(format!("{}.prev", tool.bin_name()))
}

struct BusyGuard<'a>(&'a ToolsState, Tool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.1);
    }
}

fn mark_busy(st: &AppState, tool: Tool) -> AppResult<BusyGuard<'_>> {
    let mut busy = st.tools.busy.lock().unwrap_or_else(|e| e.into_inner());
    if !busy.insert(tool) {
        return Err(AppError::invalid(format!("{} 正在安装中，请稍候。", tool.id())));
    }
    Ok(BusyGuard(&st.tools, tool))
}

/// 用新文件替换组件：旧版本保留为 .prev，便于回退。
fn swap_in(st: &AppState, tool: Tool, new_file: &Path) -> AppResult<()> {
    let target = st.tools_dir.join(tool.bin_name());
    if target.is_file() {
        let prev = prev_path(st, tool);
        let _ = std::fs::remove_file(&prev);
        std::fs::rename(&target, &prev)?;
    }
    std::fs::rename(new_file, &target)?;
    if tool == Tool::YtDlp {
        *st.tools.extractors.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    Ok(())
}

async fn fetch_text(st: &AppState, urls: &[String]) -> AppResult<String> {
    let settings = st.settings();
    let mut last_err = None;
    for u in urls {
        let client = st.net.clients_for(&settings.network, u)?.api;
        match client.get(u).header("User-Agent", "ClearClip").send().await {
            Ok(r) if r.status().is_success() => return Ok(r.text().await?),
            Ok(r) => last_err = Some(AppError::from_status(r.status().as_u16(), "下载源")),
            Err(e) => last_err = Some(e.into()),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::new(ErrorKind::Network, "没有可用的下载源")))
}

/// 并发探测各个地址，返回最先响应的一个。
async fn fastest(st: &AppState, urls: &[String]) -> Option<String> {
    // 只有一个来源时不必探测，直接下载（失败时由下载本身报错）
    if urls.len() == 1 {
        return urls.first().cloned();
    }
    let settings = st.settings();
    let mut set = tokio::task::JoinSet::new();
    for u in urls {
        let Ok(c) = st.net.clients_for(&settings.network, u) else { continue };
        let u = u.clone();
        set.spawn(async move {
            let ok = tokio::time::timeout(Duration::from_secs(8), c.api.get(&u).header("Range", "bytes=0-0").header("User-Agent", "ClearClip").send())
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|r| r.status().is_success());
            ok.then_some(u)
        });
    }
    while let Some(r) = set.join_next().await {
        if let Ok(Some(u)) = r {
            set.abort_all();
            return Some(u);
        }
    }
    None
}

/// 下载并安装（或更新）组件。
pub async fn install(app: &AppHandle, tool: Tool) -> AppResult<ToolStatus> {
    let st = app.state::<std::sync::Arc<AppState>>().inner().clone();
    let _guard = mark_busy(&st, tool)?;
    let settings = st.settings();
    let src = source_for(tool, &platform_key(), &settings.ffmpeg_edition)
        .ok_or_else(|| AppError::unsupported(format!("当前系统不支持自动下载 {}，请手动安装后导入。", tool.id())))?;
    let emit = |stage: &'static str, received: u64, total: Option<u64>| {
        let _ = app.emit(EVT_PROGRESS, ToolProgress { tool: tool.id(), stage, received, total });
    };

    emit("prepare", 0, None);
    let sums = fetch_text(&st, &candidates(&src.sums_url, &settings.component_mirrors)).await.map_err(|e| e.context("获取校验文件失败："))?;
    let expected = parse_sums(&sums, &src.asset).ok_or_else(|| AppError::msg(format!("校验文件里没有 {}", src.asset)))?;

    let urls = candidates(&src.file_url, &settings.component_mirrors);
    let url = fastest(&st, &urls).await.ok_or_else(|| AppError::new(ErrorKind::Network, "所有下载源都无法访问。请检查网络或在设置里添加 GitHub 镜像。"))?;
    log::info!("installing {} from {}", tool.id(), crate::diagnostics::strip_query(&url));

    let dl_dir = st.tools_dir.join(".download");
    std::fs::create_dir_all(&dl_dir)?;
    let part = dl_dir.join(format!("{}.part", src.asset));
    let client = st.net.clients_for(&settings.network, &url)?.download;
    let (_tx, mut rx) = tokio::sync::watch::channel(CTRL_RUN);
    let mut meta = ResumeMeta::default();
    let opts = HttpOptions { segments: 1, ..Default::default() };
    http_engine::download(&client, &HttpRequest::new(&url).header("User-Agent", "ClearClip"), &part, &mut meta, &mut rx, &opts, |r, t, _| {
        emit("download", r, t)
    })
    .await
    .map_err(|e| AppError::new(e.kind(), format!("下载 {} 失败：{e}", tool.id())))?;

    emit("verify", 0, None);
    let actual = sha256_file(&part)?;
    if actual != expected {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::msg(format!("{} 校验失败（文件可能被篡改或下载不完整），已删除。请重试或换一个下载源。", tool.id())));
    }

    emit("extract", 0, None);
    let new_file = st.tools_dir.join(format!("{}.new", tool.bin_name()));
    let (part2, new2, bin) = (part.clone(), new_file.clone(), tool.bin_name());
    tokio::task::spawn_blocking(move || extract_binary(&part2, src.archive, bin, &new2)).await.map_err(|e| AppError::msg(e.to_string()))??;
    let _ = std::fs::remove_file(&part);
    if version_of(tool, &new_file).await.is_none() {
        let _ = std::fs::remove_file(&new_file);
        return Err(AppError::msg(format!("{} 下载完成但无法运行，可能与当前系统不兼容。", tool.id())));
    }
    swap_in(&st, tool, &new_file)?;
    emit("done", 0, None);
    drop(_guard);
    Ok(status(&st, tool).await)
}

/// 回退到上一个版本。
pub async fn rollback(st: &AppState, tool: Tool) -> AppResult<ToolStatus> {
    let prev = prev_path(st, tool);
    if !prev.is_file() {
        return Err(AppError::invalid("没有可回退的版本。"));
    }
    let target = st.tools_dir.join(tool.bin_name());
    let tmp = st.tools_dir.join(format!("{}.swap", tool.bin_name()));
    if target.is_file() {
        std::fs::rename(&target, &tmp)?;
    }
    std::fs::rename(&prev, &target)?;
    if tmp.is_file() {
        std::fs::rename(&tmp, &prev)?;
    }
    if tool == Tool::YtDlp {
        *st.tools.extractors.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    Ok(status(st, tool).await)
}

/// 导入用户自己下载的组件程序。
pub async fn import(st: &AppState, tool: Tool, path: &Path) -> AppResult<ToolStatus> {
    let _guard = mark_busy(st, tool)?;
    if version_of(tool, path).await.is_none() {
        return Err(AppError::invalid(format!("这个文件不是可运行的 {}，请确认选择了正确的程序。", tool.id())));
    }
    std::fs::create_dir_all(&st.tools_dir)?;
    let new_file = st.tools_dir.join(format!("{}.new", tool.bin_name()));
    std::fs::copy(path, &new_file)?;
    make_executable(&new_file);
    swap_in(st, tool, &new_file)?;
    drop(_guard);
    Ok(status(st, tool).await)
}

/// yt-dlp 当前版本支持的提取器列表（缓存）。
pub async fn list_extractors(st: &AppState) -> AppResult<Vec<String>> {
    if let Some(list) = st.tools.extractors.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(list);
    }
    let bin = resolve(st, Tool::YtDlp).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要先安装 yt-dlp。"))?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["--ignore-config", "--list-extractors"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output()).await.map_err(|_| AppError::msg("yt-dlp 响应超时"))??;
    let list: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    *st.tools.extractors.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_edition_picks_the_gpl_or_lgpl_build() {
        let lite = source_for(Tool::Ffmpeg, "windows-x86_64", "lite").unwrap();
        let full = source_for(Tool::Ffmpeg, "windows-x86_64", "full").unwrap();
        assert!(lite.asset.ends_with("win64-lgpl.zip") && full.asset.ends_with("win64-gpl.zip"));
        assert!(full.file_url.ends_with("ffmpeg-master-latest-win64-gpl.zip") && full.sums_url.ends_with("checksums.sha256"));
        assert!(source_for(Tool::Ffmpeg, "linux-aarch64", "full").unwrap().asset.ends_with("linuxarm64-gpl.tar.xz"));
        assert!(source_for(Tool::Ffmpeg, "macos-aarch64", "full").is_none(), "macOS 没有官方构建");
        // 未知的取值按精简版处理；yt-dlp 不受影响
        assert!(source_for(Tool::Ffmpeg, "linux-x86_64", "weird").unwrap().asset.contains("lgpl"));
        assert_eq!(source_for(Tool::YtDlp, "windows-x86_64", "full").unwrap().asset, "yt-dlp.exe");
    }

    #[test]
    fn extra_dirs_cover_homebrew_on_macos() {
        let d = extra_dirs();
        match std::env::consts::OS {
            "macos" => assert!(d.iter().any(|p| p.ends_with("homebrew/bin"))),
            "linux" => assert!(d.iter().any(|p| p.ends_with("local/bin"))),
            _ => {}
        }
    }

    #[test]
    fn sources_per_platform() {
        let s = source_for(Tool::YtDlp, "windows-x86_64", "lite").unwrap();
        assert_eq!(s.file_url, "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe");
        assert_eq!(s.archive, Archive::None);
        let f = source_for(Tool::Ffmpeg, "linux-x86_64", "lite").unwrap();
        assert!(f.asset.ends_with("lgpl.tar.xz"), "only LGPL ffmpeg builds");
        assert_eq!(f.archive, Archive::TarXz);
        assert!(source_for(Tool::Ffmpeg, "macos-aarch64", "lite").is_none());
        assert!(source_for(Tool::YtDlp, "macos-aarch64", "lite").is_some());
    }

    #[test]
    fn mirror_candidates() {
        let c = candidates("https://github.com/a/b", &["https://m1/".into()]);
        assert_eq!(c, vec!["https://m1/https://github.com/a/b", "https://github.com/a/b"]);
    }

    #[test]
    fn sums_parsing() {
        let text = "1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6  yt-dlp\n58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a  yt-dlp_linux\nbad  yt-dlp.exe\n";
        assert_eq!(parse_sums(text, "yt-dlp_linux").as_deref(), Some("58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a"));
        assert_eq!(parse_sums(text, "yt-dlp.exe"), None, "malformed hash rejected");
        assert_eq!(parse_sums(text, "nope"), None);
    }

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version(Tool::Ffmpeg, "ffmpeg version N-117000-g1234 Copyright (c) 2000-2026"), "N-117000-g1234");
        assert_eq!(parse_version(Tool::YtDlp, "2026.09.30"), "2026.09.30");
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-tools-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extracts_from_zip() {
        let d = tmpdir("zip");
        let zp = d.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let o: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            w.start_file("ffmpeg-x/bin/ffprobe.exe", o).unwrap();
            w.write_all(b"probe").unwrap();
            w.start_file("ffmpeg-x/bin/ffmpeg.exe", o).unwrap();
            w.write_all(b"FFMPEG").unwrap();
            w.finish().unwrap();
        }
        let out = d.join("ffmpeg.out");
        extract_binary(&zp, Archive::Zip, "ffmpeg.exe", &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"FFMPEG");
        assert!(extract_binary(&zp, Archive::Zip, "missing", &d.join("x")).is_err());
    }

    #[test]
    fn extracts_from_tar_xz() {
        let d = tmpdir("txz");
        let tar_path = d.join("a.tar");
        {
            let mut b = tar::Builder::new(std::fs::File::create(&tar_path).unwrap());
            let data = b"ELFDATA";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "ffmpeg-x/bin/ffmpeg", &data[..]).unwrap();
            b.finish().unwrap();
        }
        let xz = d.join("a.tar.xz");
        lzma_rs::xz_compress(&mut BufReader::new(std::fs::File::open(&tar_path).unwrap()), &mut std::fs::File::create(&xz).unwrap()).unwrap();
        let out = d.join("ffmpeg.out");
        extract_binary(&xz, Archive::TarXz, "ffmpeg", &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"ELFDATA");
        #[cfg(unix)]
        assert!(is_executable(&out));
    }

    #[test]
    fn sha256_of_file() {
        let d = tmpdir("sha");
        std::fs::write(d.join("f"), b"abc").unwrap();
        assert_eq!(sha256_file(&d.join("f")).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn finds_shell_in_path() {
        if cfg!(unix) {
            assert!(find_in_path("sh").is_some());
        }
        assert!(find_in_path("definitely-not-a-real-binary-xyz").is_none());
    }
}
