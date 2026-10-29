pub mod ai;
pub mod ai_cmds;
pub mod api;
pub mod backup;
pub mod cli;
mod clipboard;
mod commands;
pub mod console;
pub mod cookies;
pub mod db;
pub mod diagnostics;
pub mod download;
pub mod edit;
pub mod engine;
pub mod error;
pub mod grab;
pub mod i18n;
pub mod inbox;
pub mod lan;
pub mod library;
pub mod library_cmds;
pub mod library_media;
pub mod live;
pub mod lut;
pub mod media_cmds;
pub mod media_server;
pub mod media_tools;
pub mod model;
pub mod naming;
pub mod net;
pub mod notify;
pub mod organize;
pub mod phone;
pub mod player;
pub mod postprocess;
pub mod power;
pub mod providers;
pub mod quality;
pub mod rules;
pub mod safebox;
pub mod secret;
pub mod security;
pub mod security_cmds;
pub mod settings;
pub mod subs;
pub mod subtitle;
pub mod subtitle_io;
pub mod subtitle_tools;
pub mod tools;
pub mod track;
pub mod transfer;
mod tray;
pub mod upload;
pub mod vault;
pub mod vidcaps;
pub mod vidnorm;
mod winframe;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tauri::{Emitter, Manager, WindowEvent};

use crate::cookies::CookieStore;
use crate::db::Db;
use crate::download::DownloadManager;
use crate::settings::Settings;

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    pub db: Db,
    /// 解析用（带总超时）
    pub client: reqwest::Client,
    /// 下载用（只有读超时，适合大文件）
    pub dl_client: reqwest::Client,
    pub downloads: DownloadManager,
    /// 托盘里临时暂停剪贴板监听
    pub clipboard_paused: AtomicBool,
    /// 程序自己写入剪贴板的内容，监听时忽略
    pub clipboard_ignore: Mutex<Option<String>>,
    pub cookies: CookieStore,
    pub samples_dir: PathBuf,
    pub log_dir: PathBuf,
    /// 最近一次注册全局快捷键失败的原因
    pub shortcut_error: Mutex<Option<String>>,
    /// 按网站分流的网络客户端、限速与请求间隔
    pub net: Arc<net::NetManager>,
    /// 程序管理的外部组件目录（yt-dlp、ffmpeg）
    pub tools_dir: PathBuf,
    pub tools: tools::ToolsState,
    pub data_dir: PathBuf,
    pub phone: phone::PhoneState,
    /// 全部下载完成后的动作：none / sleep / shutdown（不保存，每次启动为 none）
    pub after_all_done: Mutex<String>,
    pub subs: subs::SubsState,
    pub live: live::LiveState,
    /// 媒体工具箱的后台任务
    pub media_jobs: media_tools::MediaJobs,
    /// 加密保存的密钥（API 密钥、密码等）
    pub vault: vault::Vault,
    /// 应用锁状态
    pub lock: security::LockState,
    /// 加密保险箱
    pub safebox: safebox::Safebox,
    /// 内置播放器用的本机媒体服务
    pub media: media_server::MediaServer,
    /// ffmpeg 的能力（滤镜、编码器）检测结果缓存
    pub vcaps: vidcaps::CapsCache,
}

impl AppState {
    /// 实际生效的设置（省流量模式下会调低并发和清晰度）。读写设置页要用 [`settings_raw`](Self::settings_raw)。
    pub fn settings(&self) -> Settings {
        self.settings_raw().metered_view()
    }

    /// 保存的设置原样（不含省流量模式的临时调整）。
    pub fn settings_raw(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 解析用的上下文；开启“录制样本”时附带样本目录。
    pub fn parse_ctx<'a>(&'a self, settings: &'a Settings) -> providers::Ctx<'a> {
        let mut ctx = providers::Ctx::new(&self.client, settings, &self.cookies);
        ctx.net = Some(&self.net);
        if settings.record_samples {
            ctx.samples = Some(self.samples_dir.clone());
        }
        if settings.use_ytdlp {
            ctx.ytdlp = tools::resolve(self, tools::Tool::YtDlp);
        }
        ctx.ffmpeg = tools::resolve(self, tools::Tool::Ffmpeg);
        ctx
    }
}

/// 定期检查即将过期的登录 Cookie 并发送通知（每个账号每天最多提醒一次）。
fn spawn_account_reminders(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut notified: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            let st = app.state::<Arc<AppState>>().inner().clone();
            // 定期检查登录状态：从有效变为失效时提醒（网络错误不算失效）
            for a in st.cookies.summaries().into_iter().filter(|a| a.checkable) {
                if let Ok(status) = commands::verify_account(&st, &a.id).await {
                    if !status.logged_in && a.valid != Some(false) {
                        use tauri_plugin_notification::NotificationExt;
                        let body = format!("{}账号“{}”的登录已失效，需要登录的内容将无法下载。请在“设置 → 账号与 Cookie”中重新登录。", a.site_name, a.label);
                        let _ = app.notification().builder().title("登录已失效").body(&body).show();
                        notify::emit(&app, notify::Event::Account, "登录已失效", &body);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            let _ = app.emit("accounts://updated", ());
            let now = db::now();
            for a in st.cookies.summaries() {
                let Some(exp) = a.expires_at else { continue };
                let left = exp - now;
                if left > 3 * 86400 || notified.get(&a.id).is_some_and(|t| now - t < 86400) {
                    continue;
                }
                let body = if left <= 0 {
                    format!("{}账号“{}”的登录状态已过期，需要登录的内容将无法下载。", a.site_name, a.label)
                } else {
                    format!("{}账号“{}”的登录状态将在 {} 小时后过期。", a.site_name, a.label, left / 3600 + 1)
                };
                use tauri_plugin_notification::NotificationExt;
                let _ = app.notification().builder().title("登录即将失效").body(&body).show();
                notify::emit(&app, notify::Event::Account, "登录即将失效", &body);
                notified.insert(a.id.clone(), now);
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// 构建所对应的提交（短哈希），由 build.rs 写入。
pub const COMMIT: &str = env!("CLEARCLIP_COMMIT");

/// 便携模式：程序目录下有 `portable` 文件（或 `data` 目录）时，设置、数据和日志都保存在程序目录的 `data` 下。
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    (dir.join("portable").exists() || dir.join("data").is_dir()).then(|| dir.join("data"))
}

pub fn run() {
    // 命令行子命令（list / status / pause …）：执行完就退出，不启动界面
    if let Some(code) = cli::try_run() {
        std::process::exit(code);
    }
    let portable = portable_dir();
    let log_target = match &portable {
        Some(p) => tauri_plugin_log::TargetKind::Folder { path: p.join("logs"), file_name: Some("clearclip".into()) },
        None => tauri_plugin_log::TargetKind::LogDir { file_name: Some("clearclip".into()) },
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `clearclip add <链接>` 交给已经在运行的这一个
            match cli::add_text(&args) {
                Some(text) => {
                    let _ = phone::receive_trusted(app, "cli", "cli", "命令行", text);
                }
                None => tray::show_main(app),
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout), tauri_plugin_log::Target::new(log_target)])
                .level(log::LevelFilter::Info)
                .max_file_size(2 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let panic = app
                            .try_state::<Arc<AppState>>()
                            .and_then(|st| st.settings_raw().security.panic_shortcut.trim().parse::<tauri_plugin_global_shortcut::Shortcut>().ok());
                        if panic.as_ref() == Some(shortcut) {
                            security::panic(app);
                        } else {
                            clipboard::parse_clipboard_now(app);
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let (config_dir, data_dir) = match portable_dir() {
                Some(p) => (p.clone(), p),
                None => (app.path().app_config_dir()?, app.path().app_data_dir()?),
            };
            let default_dl = app.path().download_dir().or_else(|_| app.path().home_dir()).unwrap_or_else(|_| PathBuf::from(".")).join("ClearClip");
            let settings_path = config_dir.join("settings.json");
            // 上次选择了“还原备份”：在打开数据库之前替换数据库和设置
            if backup::apply_pending(&data_dir, &settings_path) {
                log::info!("restored a backup");
            }
            let mut settings = Settings::load(&settings_path, &default_dl);
            let db = Db::open(&data_dir.join("clearclip.db")).map_err(|e| e.to_string())?;
            let log_dir = if portable_dir().is_some() { data_dir.join("logs") } else { app.path().app_log_dir().unwrap_or_else(|_| data_dir.join("logs")) };
            log::info!(
                "ClearClip {} ({COMMIT}{}) starting on {}, identifier {}",
                app.package_info().version,
                if cfg!(debug_assertions) { ", debug" } else { "" },
                std::env::consts::OS,
                app.config().identifier
            );
            // 调试版：标题栏写上版本和提交号，和正式版、其他调试包一眼就能分开
            if cfg!(debug_assertions) {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_title(&format!("清影 ClearClip 调试版 {} · {COMMIT}", app.package_info().version));
                }
            }

            let key = secret::load_or_create_key(&data_dir);
            let cookie_store = CookieStore::open(cookies::store_path(&data_dir), key.key, key.in_keyring);
            if !settings.cookies.is_empty() {
                match cookie_store.migrate_legacy(&settings.cookies) {
                    Ok(n) => {
                        log::info!("migrated {n} legacy cookie entries into the encrypted store");
                        settings.cookies.clear();
                        settings.cookie_updated_at.clear();
                        let _ = settings.save(&settings_path);
                    }
                    Err(e) => log::warn!("legacy cookie migration failed: {e}"),
                }
            }

            let vault = vault::Vault::open(data_dir.join("vault.bin"), key.key);
            // 设置了应用锁的，每次启动都先锁住
            let locked = vault.has(security::APPLOCK);
            // 隐私模式：上次异常退出时没来得及清理的记录，启动时补清
            if settings.security.privacy_mode {
                security::purge_private(&db);
            }
            // 保险箱解密出来临时查看的文件，上次没清掉的也一并清理
            safebox::shred_dir(&data_dir.join(security::VIEW_DIR));
            let (media, media_listener) = media_server::MediaServer::bind().map_err(|e| format!("无法启动播放器的本机服务：{e}"))?;
            tauri::async_runtime::spawn(media.clone().serve(media_listener));
            app.manage(Arc::new(AppState {
                settings: RwLock::new(settings.clone()),
                settings_path,
                db,
                client: providers::build_client(),
                dl_client: download::build_download_client(),
                downloads: DownloadManager::default(),
                clipboard_paused: AtomicBool::new(false),
                clipboard_ignore: Mutex::new(None),
                cookies: cookie_store,
                samples_dir: diagnostics::samples_dir(&data_dir),
                log_dir,
                shortcut_error: Mutex::new(None),
                net: Arc::new(net::NetManager::default()),
                tools_dir: data_dir.join("tools"),
                tools: tools::ToolsState::default(),
                data_dir: data_dir.clone(),
                phone: phone::PhoneState::default(),
                after_all_done: Mutex::new("none".into()),
                subs: subs::SubsState::default(),
                live: live::LiveState::default(),
                media_jobs: media_tools::MediaJobs::default(),
                vault,
                lock: security::LockState::new(locked),
                safebox: safebox::Safebox::new(data_dir.join("safebox")),
                media,
                vcaps: vidcaps::CapsCache::default(),
            }));
            // 媒体库封面缓存通过 asset 协议显示
            let covers = data_dir.join("covers");
            let _ = std::fs::create_dir_all(&covers);
            let _ = app.asset_protocol_scope().allow_directory(&covers, false);
            let previews = data_dir.join("previews");
            let _ = std::fs::create_dir_all(&previews);
            let _ = app.asset_protocol_scope().allow_directory(&previews, false);
            download::restore(&handle);
            if let Ok(n) = app.state::<Arc<AppState>>().db.inbox_prune(settings.inbox.keep_days, settings.inbox.max_items) {
                if n > 0 {
                    log::info!("pruned {n} old inbox items");
                }
            }
            download::spawn_timer(&handle);
            // 限速计划：按时段切换限速
            {
                let st = app.state::<Arc<AppState>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        st.net.refresh_limit(&st.settings_raw().speed_schedule);
                        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    }
                });
            }
            library_cmds::spawn_maintenance(&handle);
            security::spawn_idle_watch(&handle);
            security::apply_window_protection(&handle, settings.security.content_protection);
            tray::apply_float(&handle, settings.float_ball && !locked);
            spawn_account_reminders(handle.clone());

            tray::create(&handle)?;
            tray::spawn_speed_ticker(&handle);
            commands::apply_shortcut(&handle, &settings.shortcut);
            clipboard::start_watcher(handle.clone());
            transfer::restore(&handle);
            phone::restore(&handle);
            subs::spawn_scheduler(&handle);
            live::spawn_monitor(&handle);
            // `clearclip add <链接>` 启动的：等界面和任务恢复好后加入下载
            if let Some(text) = cli::add_text(&std::env::args().collect::<Vec<_>>()) {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = phone::receive_trusted(&h, "cli", "cli", "命令行", text);
                });
            }
            // 开机自启时只在托盘运行
            if std::env::args().any(|a| a == "--autostart") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 迷你窗、悬浮拖拽窗是按需创建的，关闭就是销毁，不用拦截
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let app = window.app_handle();
                    let to_tray = app.state::<Arc<AppState>>().settings().close_to_tray;
                    if to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                        security::on_hide(app);
                    } else {
                        app.exit(0);
                    }
                }
            }
        })
        // 构建时提供了签名公钥才启用程序内更新；否则注册一个空插件占位（读取同名配置，但不做任何事）
        .plugin(match commands::UPDATER_PUBKEY.filter(|k| !k.is_empty()) {
            Some(key) => tauri_plugin_updater::Builder::new().pubkey(key).build(),
            None => tauri::plugin::Builder::<tauri::Wry, tauri_plugin_updater::Config>::new("updater").build(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::save_settings,
            commands::detect_links,
            commands::resolve_link,
            commands::resolve_and_enqueue,
            commands::enqueue,
            commands::enqueue_entries,
            commands::tools_status,
            commands::install_tool,
            commands::rollback_tool,
            commands::import_tool,
            commands::list_extractors,
            commands::open_tools_dir,
            commands::make_slideshow,
            commands::live_rooms,
            commands::live_check,
            commands::live_add,
            commands::live_update,
            commands::live_set_monitoring,
            commands::live_delete,
            commands::live_start,
            commands::live_stop,
            commands::live_recordings,
            commands::subs_list,
            commands::subs_preview,
            commands::subs_add,
            commands::subs_update,
            commands::subs_set_paused,
            commands::subs_delete,
            commands::subs_check,
            commands::subs_items,
            commands::subs_download_items,
            commands::subs_ignore_items,
            commands::subs_clear_new,
            commands::library_platforms,
            commands::redownload,
            commands::health_check,
            commands::site_rule_test,
            commands::move_task,
            commands::schedule_task,
            commands::get_after_all_done,
            commands::set_after_all_done,
            commands::read_links_file,
            commands::is_portable,
            commands::phone_info,
            commands::phone_enable,
            commands::phone_reset_token,
            commands::phone_revoke,
            commands::phone_pair_respond,
            commands::phone_set_options,
            commands::transfer_list,
            commands::transfer_send_files,
            commands::transfer_send_text,
            commands::transfer_remove,
            commands::transfer_clear,
            commands::list_tasks,
            commands::task_detail,
            commands::pause_task,
            commands::resume_task,
            commands::cancel_task,
            commands::remove_task,
            commands::clear_finished,
            commands::pause_all,
            commands::resume_all,
            commands::login_site,
            commands::preferred_subtitles,
            commands::downloaded_entries,
            commands::inbox_list,
            commands::inbox_counts,
            commands::inbox_retry,
            commands::inbox_retry_failed,
            commands::inbox_ignore,
            commands::inbox_delete,
            commands::inbox_clear,
            commands::list_history,
            commands::delete_history,
            commands::clear_history,
            commands::list_library,
            commands::delete_library,
            ai_cmds::notify_test,
            ai_cmds::upload_test,
            library_cmds::library_set_meta,
            library_cmds::library_set_tags,
            library_cmds::library_bulk_tags,
            library_cmds::library_bulk_favorite,
            library_cmds::library_tags,
            library_cmds::library_delete,
            library_cmds::library_remove_missing,
            library_cmds::library_import,
            library_cmds::library_stats,
            library_cmds::library_preview,
            library_cmds::library_scenes,
            library_cmds::library_split_scenes,
            library_cmds::library_export,
            library_cmds::trash_list,
            library_cmds::trash_restore,
            library_cmds::trash_purge,
            library_cmds::trash_empty,
            library_cmds::reorganize_plan,
            library_cmds::reorganize_apply,
            library_cmds::duplicates_exact,
            library_cmds::duplicates_similar,
            library_cmds::cues_search,
            library_cmds::cues_reindex,
            library_cmds::backup_export,
            library_cmds::backup_import,
            library_cmds::restart_app,
            ai_cmds::secret_set,
            ai_cmds::secret_has,
            ai_cmds::ai_status,
            ai_cmds::ai_test,
            ai_cmds::ai_translate,
            ai_cmds::ai_summarize,
            ai_cmds::ai_transcribe,
            ai_cmds::media_job_text,
            media_cmds::media_job_start,
            media_cmds::media_jobs,
            media_cmds::media_job_cancel,
            media_cmds::media_jobs_clear,
            media_cmds::media_info,
            media_cmds::video_caps,
            media_cmds::video_presets,
            media_cmds::video_analyze,
            media_cmds::video_color,
            media_cmds::video_preview,
            edit::cmds::edit_probe,
            edit::cmds::edit_thumb,
            edit::cmds::edit_preview_url,
            edit::cmds::edit_preview_start,
            edit::cmds::edit_save,
            edit::cmds::edit_open,
            edit::cmds::edit_missing,
            edit::cmds::edit_proxy,
            edit::cmds::edit_subtitle_cues,
            edit::cmds::edit_fonts,
            track::cmds::track_detect,
            track::cmds::track_follow,
            track::cmds::track_cancel,
            lut::cmds::lut_looks,
            lut::cmds::lut_inspect,
            lut::cmds::lut_sample,
            lut::cmds::lut_preview,
            lut::cmds::lut_save,
            lut::cmds::lut_export_image,
            media_cmds::subtitle_tool,
            commands::list_orphan_parts,
            commands::delete_orphan_parts,
            commands::list_accounts,
            commands::import_cookies_file,
            commands::import_cookies_text,
            commands::rename_account,
            commands::set_default_account,
            commands::delete_account,
            commands::check_account,
            commands::test_route,
            commands::route_speedtest,
            commands::get_diagnostics,
            commands::open_log_dir,
            commands::open_samples_dir,
            commands::copy_text,
            commands::open_file,
            commands::reveal_file,
            commands::open_url,
            commands::show_main,
            commands::hide_mini,
            commands::mini_ready,
            commands::open_login,
            commands::save_login_cookies,
            commands::check_update,
            commands::install_update,
            player::player_source,
            security_cmds::lock_status,
            security_cmds::lock_set_password,
            security_cmds::lock_remove_password,
            security_cmds::lock_now,
            security_cmds::lock_unlock,
            security_cmds::wipe_traces,
            security_cmds::safebox_status,
            security_cmds::safebox_create,
            security_cmds::safebox_unlock,
            security_cmds::safebox_lock,
            security_cmds::safebox_change_password,
            security_cmds::safebox_list,
            security_cmds::safebox_add_files,
            security_cmds::safebox_add_library,
            security_cmds::safebox_open,
            security_cmds::safebox_export,
            security_cmds::safebox_remove,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ClearClip")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<Arc<AppState>>() {
                    security::seal_safebox(&st);
                    if st.settings_raw().security.privacy_mode {
                        security::purge_private(&st.db);
                    }
                }
            }
        });
}
