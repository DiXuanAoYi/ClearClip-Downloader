use super::*;

fn cue(s: u64, e: u64, t: &str) -> Cue {
    Cue { start: s, end: e, lines: vec![t.into()] }
}

fn kinds(c: &[Cue]) -> Vec<(usize, &'static str)> {
    validate(c).into_iter().map(|i| (i.index, i.kind)).collect()
}

#[test]
fn validate_flags_timing_and_text_problems() {
    assert!(kinds(&[cue(0, 2000, "你好世界"), cue(2000, 4000, "再见")]).is_empty());
    assert_eq!(kinds(&[cue(2000, 1000, "倒了")]), vec![(0, "reversed")]);
    assert_eq!(kinds(&[cue(0, 2000, "  ")]), vec![(0, "empty")]);
    assert_eq!(kinds(&[cue(0, 12_000, "太久")]), vec![(0, "too-long")]);
    assert_eq!(kinds(&[cue(0, 100, "闪")]), vec![(0, "too-short")]);
    assert_eq!(kinds(&[cue(0, 1000, "这一句话字数实在太多了根本读不过来的呀呀呀呀呀")]), vec![(0, "too-fast")]);
    let k = kinds(&[cue(0, 2000, "第一句话"), cue(1500, 3500, "第二句话")]);
    assert_eq!(k, vec![(1, "overlap")]);
}

#[test]
fn save_copy_keeps_original_and_overwrite_backs_up_once() {
    let dir = std::env::temp_dir().join(format!("subedit-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("a.zh.srt");
    std::fs::write(&p, subtitle::format_srt(&[cue(0, 1000, "原文")])).unwrap();
    let edited = vec![cue(2000, 3000, "后"), cue(0, 1000, "先")];

    let r = save_cues(&p, &edited, false, None).unwrap();
    assert!(r.path.ends_with("a.zh.已编辑.srt"), "{}", r.path);
    assert!(std::fs::read_to_string(&p).unwrap().contains("原文"));
    // 保存时按时间排序
    let saved = std::fs::read_to_string(&r.path).unwrap();
    assert!(saved.find("先").unwrap() < saved.find("后").unwrap());
    // 再存一份不会覆盖上一份
    assert!(save_cues(&p, &edited, false, None).unwrap().path.ends_with("a.zh.已编辑.1.srt") || save_cues(&p, &edited, false, None).unwrap().path != r.path);

    let o = save_cues(&p, &edited, true, None).unwrap();
    assert_eq!(o.path, p.to_string_lossy());
    let bak = o.backup.expect("first overwrite makes a backup");
    assert!(std::fs::read_to_string(bak).unwrap().contains("原文"));
    assert!(std::fs::read_to_string(&p).unwrap().contains("先"));
    // 第二次覆盖不再动备份
    assert!(save_cues(&p, &edited, true, None).unwrap().backup.is_none());

    assert!(save_cues(&p, &[], false, None).is_err());
    assert!(save_cues(&p, &edited, false, Some("lrc")).is_err());
    // 换格式：另存而不是覆盖
    let v = save_cues(&p, &edited, true, Some("vtt")).unwrap();
    assert!(v.path.ends_with(".vtt") && v.backup.is_none());
    assert!(std::fs::read_to_string(&v.path).unwrap().starts_with("WEBVTT"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ass_colors_are_bgr_and_bad_values_fall_back() {
    assert_eq!(ass_color("#FFD966"), "&H0066D9FF");
    assert_eq!(ass_color("ff0000"), "&H000000FF");
    assert_eq!(ass_color("nope"), "&H00FFFFFF");
}

#[test]
fn styled_ass_gives_second_line_its_own_size_and_color() {
    let c = Cue { start: 1000, end: 3000, lines: vec!["你好".into(), "Hello {x}".into()] };
    let ass = styled_ass(&[c], &preset("bilingual").unwrap(), 1920, 1080);
    assert!(ass.contains("PlayResX: 1920") && ass.contains("PlayResY: 1080"));
    assert!(ass.contains("Style: Default,Microsoft YaHei,54,"));
    assert!(ass.contains("你好\\N{\\fs44\\c&H0066D9FF}Hello ｛x｝"), "{ass}");
    assert!(ass.contains("Dialogue: 0,0:00:01.00,0:00:03.00,"));
}

#[test]
fn styled_ass_scales_with_resolution_and_supports_box_and_top() {
    let c = cue(0, 1000, "字");
    let small = styled_ass(std::slice::from_ref(&c), &BurnStyle::default(), 1280, 720);
    assert!(small.contains(",37,"), "56*720/1080 ≈ 37: {small}");
    let boxed = BurnStyle { boxed: true, position: "top".into(), ..BurnStyle::default() };
    let a = styled_ass(std::slice::from_ref(&c), &boxed, 1920, 1080);
    let style = a.lines().find(|l| l.starts_with("Style:")).unwrap();
    // BorderStyle=3 (底框)，Alignment=8 (顶部居中)
    assert!(style.contains(",3,") && style.contains(",8,"), "{style}");
    // 竖屏按宽度缩放
    let portrait = styled_ass(std::slice::from_ref(&c), &BurnStyle::default(), 720, 1280);
    assert!(portrait.contains(",37,"), "{portrait}");
}

#[test]
fn presets_exist_and_default_is_standard() {
    for (id, _) in PRESETS {
        assert!(preset(id).is_some(), "{id}");
    }
    assert!(preset("nope").is_none());
    assert_eq!(BurnStyle::default(), preset("standard").unwrap());
    assert_eq!(preset("bilingual").unwrap().color2, "#FFD966");
}

#[test]
fn serde_roundtrip_uses_camel_case() {
    let e: EditCue = serde_json::from_str(r#"{"startMs":1,"endMs":2,"lines":["a","b"]}"#).unwrap();
    assert_eq!(Cue::from(&e), Cue { start: 1, end: 2, lines: vec!["a".into(), "b".into()] });
    let o: MuxOpts = serde_json::from_str(r#"{"mode":"hard","tracks":["x.srt"],"style":{"size":70}}"#).unwrap();
    assert_eq!(o.style.unwrap().size, 70);
}
