//! 清晰度预设：解析后按设置给视频格式排序（第一个即默认选中），自动下载时决定下载哪些资源。

use crate::download::PostOptions;
use crate::model::{Asset, AssetKind, MediaInfo, MediaKind};
use crate::settings::{QualityPreset, Settings};
use crate::subtitle;

fn is_h264(a: &Asset) -> bool {
    a.vcodec.as_deref().is_some_and(|c| {
        let c = c.to_ascii_lowercase();
        c.starts_with("avc") || c.starts_with("h264")
    })
}

/// 视频格式的排序键（越大越优先）。
fn rank(a: &Asset, preset: QualityPreset, prefer_h264: bool) -> (bool, i64, bool, i64) {
    let side = a.short_side().unwrap_or(0) as i64;
    let h264 = prefer_h264 && is_h264(a);
    let bitrate = a.bitrate.unwrap_or(0) as i64;
    match preset {
        // 超过 1080P 的排到后面
        QualityPreset::Max1080 => (side <= 1080, side, h264, bitrate),
        // 不超过 720P 里分辨率最低（但不低于 360P）、码率最低的
        QualityPreset::Small => (side <= 720 && (side >= 360 || side == 0), -side, h264, -bitrate),
        QualityPreset::Best | QualityPreset::Audio => (true, side, h264, bitrate),
    }
}

/// 预估一个视频格式（含需要合并的音频轨）下载后的大小（字节）；无法估算时为 None。
pub fn estimated_size(info: &MediaInfo, a: &Asset) -> Option<u64> {
    let own = |x: &Asset| -> Option<u64> {
        x.filesize.filter(|s| *s > 0).or_else(|| match (x.bitrate, info.duration_ms) {
            (Some(kbps), Some(ms)) if kbps > 0 && ms > 0 => Some(kbps * 1000 / 8 * ms / 1000),
            _ => None,
        })
    };
    let video = own(a)?;
    let audio = a.pair_audio.as_deref().and_then(|id| info.asset(id)).and_then(own).unwrap_or(0);
    Some(video + audio)
}

/// 失败后改用哪个视频格式：按当前排序（偏好最优在前）取在失败格式之后、没有试过、且清晰度不高于它的第一个。
pub fn next_lower<'a>(info: &'a MediaInfo, failed: &Asset, tried: &[String]) -> Option<&'a Asset> {
    let videos: Vec<&Asset> = info.assets.iter().filter(|a| a.kind == AssetKind::Video).collect();
    let at = videos.iter().position(|a| a.id == failed.id)?;
    let side = failed.short_side().unwrap_or(u32::MAX);
    videos.into_iter().skip(at + 1).find(|a| !tried.contains(&a.id) && a.id != failed.id && a.short_side().unwrap_or(0) <= side)
}

/// 按预设给视频格式排序，其他资源保持原有顺序并排在视频之后。
/// 设置了“大小上限”时，默认格式（第一个）预估超过上限就换成不超过上限的最高偏好格式。
pub fn sort_videos(info: &mut MediaInfo, settings: &Settings) {
    let (preset, h264) = (settings.quality_preset, settings.prefer_h264);
    let mut videos: Vec<Asset> = info.assets.iter().filter(|a| a.kind == AssetKind::Video).cloned().collect();
    if videos.len() < 2 {
        return;
    }
    // 稳定排序：同等条件下保留平台给出的顺序
    videos.sort_by_key(|a| std::cmp::Reverse(rank(a, preset, h264)));
    if settings.max_size_mb > 0 {
        let limit = settings.max_size_mb * 1024 * 1024;
        let sizes: Vec<Option<u64>> = videos.iter().map(|a| estimated_size(info, a)).collect();
        if sizes[0].is_some_and(|s| s > limit) {
            let pick =
                sizes.iter().position(|s| s.is_some_and(|s| s <= limit)).or_else(|| (0..sizes.len()).filter(|i| sizes[*i].is_some()).min_by_key(|i| sizes[*i]));
            if let Some(i) = pick {
                let chosen = videos.remove(i);
                videos.insert(0, chosen);
            }
        }
    }
    let others = info.assets.iter().filter(|a| a.kind != AssetKind::Video).cloned();
    info.assets = videos.into_iter().chain(others).collect();
}

/// 按偏好语言挑选字幕：每个偏好语言取一条（人工字幕优先于自动生成和 AI 字幕），弹幕不在其中。
pub fn preferred_subtitles(info: &MediaInfo, settings: &Settings) -> Vec<String> {
    let lang_of = |a: &Asset| a.format_id.clone().or_else(|| a.quality.clone()).unwrap_or_default();
    let machine = |a: &Asset| a.id.starts_with("auto-") || a.quality.as_deref().is_some_and(|q| q.ends_with(".auto") || q.ends_with(".ai"));
    let subs: Vec<&Asset> = info.assets.iter().filter(|a| a.kind == AssetKind::Subtitle && a.ext != "xml").collect();
    let mut out: Vec<String> = vec![];
    for pref in &settings.subtitle_langs {
        let best = subs.iter().enumerate().filter(|(_, a)| subtitle::lang_matches(&lang_of(a), pref)).min_by_key(|(i, a)| (machine(a), *i));
        if let Some((_, a)) = best {
            if !out.contains(&a.id) {
                out.push(a.id.clone());
            }
        }
    }
    out
}

/// 自动下载（剪贴板、播放列表、订阅）时下载哪些资源，以及后处理选项。
/// “字幕处理”不为 off 时，同时带上偏好语言的字幕。
pub fn auto_selection(info: &MediaInfo, settings: &Settings) -> (Vec<String>, PostOptions) {
    let (mut ids, mut post) = base_selection(info, settings);
    if settings.subtitle_mode != "off" && ids.iter().any(|id| info.asset(id).is_some_and(|a| a.kind == AssetKind::Video)) {
        let mut subs = preferred_subtitles(info, settings);
        // 烧录进画面时多种语言会叠在一起，只用最优先的一种
        if settings.subtitle_mode == "burn" {
            subs.truncate(1);
        }
        if !subs.is_empty() {
            ids.extend(subs);
            post.sub_mode = match settings.subtitle_mode.as_str() {
                "embed" => Some("soft".into()),
                "burn" => Some("burn".into()),
                _ => None,
            };
        }
    }
    (ids, post)
}

fn base_selection(info: &MediaInfo, settings: &Settings) -> (Vec<String>, PostOptions) {
    let mut post = PostOptions::default();
    if settings.quality_preset == QualityPreset::Audio && info.kind == MediaKind::Video {
        // 优先下载单独的音频轨；没有时下载视频再提取音频
        if let Some(a) = info.assets.iter().filter(|a| a.kind == AssetKind::Audio).max_by_key(|a| a.bitrate.unwrap_or(0)) {
            if a.ext != settings.audio_format {
                post.extract_audio = Some(settings.audio_format.clone());
            }
            return (vec![a.id.clone()], post);
        }
        post.extract_audio = Some(settings.audio_format.clone());
    }
    (info.default_asset_ids(), post)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, side: u32, codec: &str, br: u64) -> Asset {
        let mut a = Asset::base(id, AssetKind::Video, String::new(), id, "mp4");
        a.width = Some(side * 16 / 9);
        a.height = Some(side);
        a.vcodec = Some(codec.into());
        a.bitrate = Some(br);
        a
    }

    fn info() -> MediaInfo {
        let mut audio = Asset::base("a", AssetKind::Audio, String::new(), "a", "m4a");
        audio.bitrate = Some(128);
        let assets = vec![
            v("v360", 360, "avc1", 500),
            Asset::cover("c.jpg".into()),
            v("v2160", 2160, "vp9", 9000),
            v("v1080v", 1080, "vp9", 2000),
            v("v1080h", 1080, "avc1", 3000),
            v("v720", 720, "avc1", 1500),
            audio,
        ];
        MediaInfo {
            platform: "x".into(),
            platform_name: "x".into(),
            id: "1".into(),
            source_url: String::new(),
            title: "t".into(),
            author: String::new(),
            cover: None,
            duration_ms: None,
            kind: MediaKind::Video,
            width: None,
            height: None,
            published_at: None,
            assets,
            entries: vec![],
            series: None,
            chapters: vec![],
            extractor: None,
        }
    }

    fn first(preset: QualityPreset, h264: bool) -> String {
        let mut i = info();
        let s = Settings { quality_preset: preset, prefer_h264: h264, ..Settings::default() };
        sort_videos(&mut i, &s);
        assert_eq!(i.assets.len(), 7);
        i.assets[0].id.clone()
    }

    #[test]
    fn size_limit_picks_the_best_format_that_fits() {
        let mut i = info();
        // 给每个格式一个预估大小：2160P 800 MB、1080P 300 MB / 250 MB、720P 120 MB、360P 40 MB
        for a in &mut i.assets {
            a.filesize = match a.id.as_str() {
                "v2160" => Some(800 << 20),
                "v1080h" => Some(300 << 20),
                "v1080v" => Some(250 << 20),
                "v720" => Some(120 << 20),
                "v360" => Some(40 << 20),
                _ => None,
            };
        }
        let first = |mb: u64| {
            let mut x = i.clone();
            sort_videos(&mut x, &Settings { max_size_mb: mb, ..Settings::default() });
            x.assets[0].id.clone()
        };
        assert_eq!(first(0), "v2160", "no limit");
        assert_eq!(first(1000), "v2160", "already fits");
        assert_eq!(first(500), "v1080h", "best preference that fits");
        assert_eq!(first(150), "v720");
        assert_eq!(first(10), "v360", "nothing fits: the smallest one");
        // 没有大小信息时不改变排序
        let mut x = info();
        sort_videos(&mut x, &Settings { max_size_mb: 1, ..Settings::default() });
        assert_eq!(x.assets[0].id, "v2160");
        // 用码率和时长估算
        let mut y = info();
        y.duration_ms = Some(100_000);
        let a = y.asset("v720").unwrap().clone();
        assert_eq!(estimated_size(&y, &a), Some(1500 * 1000 / 8 * 100));
    }

    #[test]
    fn downgrade_goes_down_never_up() {
        let mut i = info();
        sort_videos(&mut i, &Settings { quality_preset: QualityPreset::Max1080, ..Settings::default() });
        // 排序：v1080h, v1080v, v720, v360, v2160
        let ids: Vec<&str> = i.assets.iter().filter(|a| a.kind == AssetKind::Video).map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["v1080h", "v1080v", "v720", "v360", "v2160"]);
        let f = i.asset("v1080h").unwrap().clone();
        assert_eq!(next_lower(&i, &f, &[]).unwrap().id, "v1080v", "another codec at the same height first");
        assert_eq!(next_lower(&i, &f, &["v1080v".into()]).unwrap().id, "v720");
        let f = i.asset("v360").unwrap().clone();
        assert!(next_lower(&i, &f, &[]).is_none(), "2160P comes later in the list but is higher, so it is not a downgrade");
        let f = i.asset("v720").unwrap().clone();
        assert_eq!(next_lower(&i, &f, &[]).unwrap().id, "v360");
        let cover = i.assets.iter().find(|a| a.kind == AssetKind::Cover).unwrap().clone();
        assert!(next_lower(&i, &cover, &[]).is_none());
    }

    #[test]
    fn presets() {
        assert_eq!(first(QualityPreset::Best, true), "v2160");
        assert_eq!(first(QualityPreset::Max1080, true), "v1080h");
        assert_eq!(first(QualityPreset::Max1080, false), "v1080h", "higher bitrate wins without codec preference");
        assert_eq!(first(QualityPreset::Small, true), "v360");
    }

    fn with_subs(mode: &str) -> (MediaInfo, Settings) {
        let mut i = info();
        let mut sub = |id: &str, lang: &str, quality: &str| {
            let mut a = Asset::subtitle(lang, lang, String::new(), "vtt");
            a.id = id.into();
            a.quality = Some(quality.into());
            a.format_id = Some(lang.into());
            i.assets.push(a);
        };
        sub("auto-en", "en-orig", "en.auto");
        sub("sub-en", "en", "en");
        sub("sub-ja", "ja", "ja");
        sub("sub-zh-Hant", "zh-Hant", "zh-Hant");
        sub("cc-ai-zh", "ai-zh", "zh.ai");
        let mut danmaku = Asset::base("danmaku", AssetKind::Subtitle, String::new(), "弹幕", "xml");
        danmaku.quality = Some("danmaku".into());
        i.assets.push(danmaku);
        (i, Settings { subtitle_mode: mode.into(), subtitle_langs: vec!["zh".into(), "en".into()], ..Settings::default() })
    }

    #[test]
    fn subtitles_follow_preferences() {
        let (i, s) = with_subs("file");
        // 中文：繁体人工字幕优先于 AI 中文；英文：人工字幕优先于自动生成；日文和弹幕不在偏好里
        assert_eq!(preferred_subtitles(&i, &s), vec!["sub-zh-Hant", "sub-en"]);
        let (ids, post) = auto_selection(&i, &s);
        assert_eq!(ids, vec!["v360", "sub-zh-Hant", "sub-en"]);
        assert_eq!(post.sub_mode, None, "file mode keeps subtitles as separate files");
        let (i, s) = with_subs("embed");
        assert_eq!(auto_selection(&i, &s).1.sub_mode.as_deref(), Some("soft"));
        let (i, s) = with_subs("burn");
        let (ids, post) = auto_selection(&i, &s);
        assert_eq!(post.sub_mode.as_deref(), Some("burn"));
        assert_eq!(ids, vec!["v360", "sub-zh-Hant"], "only one language is burned into the picture");
        let (i, s) = with_subs("off");
        assert_eq!(auto_selection(&i, &s).0, vec!["v360"]);
        // 只有机器字幕时也会选
        let (mut i, s) = with_subs("file");
        i.assets.retain(|a| a.id != "sub-en" && a.id != "sub-zh-Hant");
        assert_eq!(preferred_subtitles(&i, &s), vec!["cc-ai-zh", "auto-en"]);
        // 偏好里没有的语言不下载
        let s = Settings { subtitle_langs: vec!["fr".into()], ..s };
        assert!(preferred_subtitles(&i, &s).is_empty());
    }

    #[test]
    fn audio_preset_selects_audio_track() {
        let s = Settings { quality_preset: QualityPreset::Audio, audio_format: "mp3".into(), ..Settings::default() };
        let (ids, post) = auto_selection(&info(), &s);
        assert_eq!(ids, vec!["a"]);
        assert_eq!(post.extract_audio.as_deref(), Some("mp3"));
        let s = Settings { audio_format: "m4a".into(), ..s };
        assert_eq!(auto_selection(&info(), &s).1.extract_audio, None);
        let (ids, post) = auto_selection(&info(), &Settings::default());
        assert_eq!(ids, vec!["v360"], "unsorted info keeps the provider order");
        assert_eq!(post, PostOptions::default());
    }
}
