use super::*;

fn cue(s: u64, e: u64, t: &str) -> Cue {
    Cue { start: s, end: e, lines: t.split('\n').map(str::to_string).collect() }
}

fn term(s: &str) -> Term {
    Term { source: s.into(), target: String::new(), note: String::new() }
}

#[test]
fn hallucinations_and_repeats_are_dropped() {
    let cues = vec![
        cue(0, 1000, "大家好欢迎来到本期视频"),
        cue(1000, 2000, "谢谢观看"),
        cue(2000, 3000, "Thanks for watching!"),
        cue(3000, 4000, "   "),
        cue(4000, 5000, "今天我们讲一件事"),
        cue(5000, 6000, "今天我们讲一件事"),
        cue(6000, 7000, "今天我们讲一件事。"),
        cue(7000, 8000, "好的"),
    ];
    let (out, removed) = filter_cues(cues);
    assert_eq!(removed, 5);
    assert_eq!(out.len(), 3);
    assert_eq!((out[1].start, out[1].end), (4000, 7000)); // 重复的折叠，时间延长
                                                          // “谢谢观看”夹在正常句子里时不算幻觉
    let (keep, _) = filter_cues(vec![cue(0, 1, "谢谢观看这个视频的朋友们")]);
    assert_eq!(keep.len(), 1);
}

#[test]
fn punctuation_density_detects_unpunctuated_asr() {
    let none = vec![cue(0, 1000, "大家好欢迎来到本期视频今天我们来聊聊如何在家里自己做一杯好喝的咖啡"), cue(1000, 2000, "首先需要准备咖啡豆和磨豆机")];
    assert!(needs_punct(&none));
    let some = vec![cue(0, 1000, "大家好，欢迎来到本期视频。今天我们来聊聊如何在家里自己做一杯好喝的咖啡，"), cue(1000, 2000, "首先需要准备咖啡豆和磨豆机。")];
    assert!(!needs_punct(&some));
    // 太短不判断
    assert!(!needs_punct(&[cue(0, 1, "好的")]));
    assert!(!needs_punct(&[]));
}

#[test]
fn rules_clean_fillers_punctuation_and_spacing() {
    let out = fix_rules(&[
        cue(0, 1, "嗯 我们用iPhone拍,然后..."),
        cue(1, 2, "the the quick brown fox"),
        cue(2, 3, "um, so we start"),
        cue(3, 4, "价格是3.5元,共1,000个"),
        cue(4, 5, "呃"),
    ]);
    assert_eq!(out.len(), 4); // 只剩口头禅的那条被去掉
    assert_eq!(out[0].lines, vec!["我们用 iPhone 拍，然后…"]);
    assert_eq!(out[1].lines, vec!["the quick brown fox"]);
    assert_eq!(out[2].lines, vec!["so we start"]);
    assert_eq!(out[3].lines, vec!["价格是 3.5 元，共 1,000 个"]); // 数字里的 . 和 , 不变
}

#[test]
fn rules_do_not_touch_time_or_meaning() {
    let src = vec![cue(1500, 4200, "我们可以看到结果是对的")];
    let out = fix_rules(&src);
    assert_eq!((out[0].start, out[0].end), (1500, 4200));
    assert_eq!(out[0].lines, src[0].lines);
}

#[test]
fn resegment_splits_long_cues_by_punctuation_and_time() {
    let long = cue(0, 10_000, "今天我们来聊聊咖啡的冲泡方法，首先要选好豆子。其次是研磨的粗细，最后是水温和时间的控制，这三点缺一不可");
    let out = resegment(std::slice::from_ref(&long), SegOpts::default());
    assert!(out.len() >= 3, "{out:?}");
    assert_eq!(out.first().unwrap().start, 0);
    assert_eq!(out.last().unwrap().end, 10_000);
    for w in out.windows(2) {
        assert_eq!(w[0].end, w[1].start); // 连续、不重叠
    }
    for c in &out {
        assert!(cue_text(c).chars().count() <= SegOpts::default().max_cjk + 6, "{}", cue_text(c));
    }
    // 不丢字（比较去掉空白后的内容）
    let joined: String = out.iter().map(cue_text).collect::<String>().replace(' ', "");
    assert_eq!(joined, cue_text(&long));
    // 短的原样保留
    let short = cue(0, 1000, "你好");
    assert_eq!(resegment(std::slice::from_ref(&short), SegOpts::default()), vec![short]);
}

#[test]
fn resegment_english_splits_at_spaces_and_keeps_decimals() {
    let t = "So the total comes to 3.5 million dollars and we still have to decide how to spend it before the end of the year";
    let out = resegment(&[cue(0, 8000, t)], SegOpts::default());
    assert!(out.len() >= 2);
    let joined = out.iter().map(cue_text).collect::<Vec<_>>().join(" ");
    assert_eq!(joined, t);
    assert!(joined.contains("3.5 million"));
}

#[test]
fn bilingual_display_modes() {
    let orig = vec![cue(0, 1000, "Hello"), cue(1000, 2000, "World")];
    let tr = vec![Some("你好".to_string()), None];
    assert_eq!(compose(&orig, &tr, Display::Orig)[0].lines, vec!["Hello"]);
    assert_eq!(compose(&orig, &tr, Display::Trans)[0].lines, vec!["你好"]);
    assert_eq!(compose(&orig, &tr, Display::OrigTop)[0].lines, vec!["Hello", "你好"]);
    assert_eq!(compose(&orig, &tr, Display::TransTop)[0].lines, vec!["你好", "Hello"]);
    // 缺译文的行显示原文，时间不变
    let b = compose(&orig, &tr, Display::Trans);
    assert_eq!(b[1].lines, vec!["World"]);
    assert_eq!((b[1].start, b[1].end), (1000, 2000));
    assert_eq!(Display::parse("trans-top"), Display::TransTop);
    assert_eq!(Display::parse("whatever"), Display::OrigTop);
    assert!(Display::TransTop.is_bilingual() && !Display::Orig.is_bilingual());
}

#[test]
fn exports_cover_every_format() {
    let cues = vec![cue(1_500, 4_250, "第一行\n第二行"), cue(65_000, 66_000, "后面")];
    assert!(export(&cues, "srt").unwrap().contains("00:00:01,500 --> 00:00:04,250"));
    assert!(export(&cues, "vtt").unwrap().starts_with("WEBVTT"));
    assert!(export(&cues, "ass").unwrap().contains("Dialogue:"));
    assert_eq!(export(&cues, "lrc").unwrap(), "[00:01.50]第一行 第二行\n[01:05.00]后面\n");
    assert_eq!(export(&cues, "txt").unwrap(), "第一行 第二行\n后面\n");
    let j: serde_json::Value = serde_json::from_str(&export(&cues, "json").unwrap()).unwrap();
    assert_eq!(j[0]["start"], 1500);
    assert_eq!(j[0]["lines"][1], "第二行");
    assert!(export(&cues, "doc").is_err());
}

#[test]
fn edit_ratio_ignores_punctuation_and_case() {
    assert_eq!(edit_ratio("大家好欢迎来到", "大家好，欢迎来到。"), 0.0);
    assert_eq!(edit_ratio("Hello world", "hello, World."), 0.0);
    assert!(edit_ratio("这是我的权利", "这是我的权力") < 0.25);
    assert!(edit_ratio("这是我的权利", "完全不同的另外一句话") > 0.7);
}

#[test]
fn guard_accepts_punctuation_and_homophones_but_rejects_rewrites() {
    let none: Vec<Term> = vec![];
    // 只补标点：总是通过
    assert_eq!(guard_line("大家好欢迎来到本期视频", "大家好，欢迎来到本期视频。", Mode::Conservative, &none), Verdict::Accept);
    // 同音字
    assert_eq!(guard_line("这是每个人的权利和义务", "这是每个人的权利和义务。", Mode::Conservative, &none), Verdict::Accept);
    assert_eq!(guard_line("他跑的很快在比赛中拿了第一", "他跑得很快，在比赛中拿了第一。", Mode::Conservative, &none), Verdict::Accept);
    // 重写：拒绝
    assert_eq!(guard_line("他跑的很快", "他是一位非常出色的运动员", Mode::Conservative, &none), Verdict::Reject("改动过大"));
    // 改了数字：保守模式拒绝
    assert_eq!(guard_line("一共有35个人", "一共有36个人", Mode::Conservative, &none), Verdict::Reject("数字被改动"));
    // 改成空
    assert_eq!(guard_line("你好", "  ", Mode::Conservative, &none), Verdict::Reject("改后为空"));
    // 单行变多行
    assert_eq!(guard_line("你好世界", "你好\n世界", Mode::Conservative, &none), Verdict::Reject("行数变了"));
}

#[test]
fn guard_protects_glossary_terms() {
    let terms = vec![term("Kubernetes")];
    assert_eq!(guard_line("we deploy on kubernetes today", "we deploy on communities today", Mode::Conservative, &terms), Verdict::Reject("术语被改动"));
    assert_eq!(guard_line("we deploy on kubernetes today", "We deploy on Kubernetes today.", Mode::Conservative, &terms), Verdict::Accept);
}

#[test]
fn apply_corrections_keeps_time_and_falls_back_per_line() {
    let orig = vec![cue(0, 1000, "大家好欢迎收看"), cue(1000, 2000, "今天讲咖啡"), cue(2000, 3000, "第三句话"), cue(3000, 4000, "第四句")];
    let texts = vec![
        Some("大家好，欢迎收看。".to_string()),             // 采用
        Some("这是一段完全重写过的很长的内容".to_string()), // 改动过大 → 保留原文
        None,                                               // 模型没给结果
        Some("第四句".to_string()),                         // 没变
    ];
    let r = apply_corrections(&orig, &texts, Mode::Conservative, &[]);
    assert_eq!(r.cues.len(), 4);
    assert_eq!(r.cues[0].lines, vec!["大家好，欢迎收看。"]);
    assert_eq!(r.cues[1].lines, orig[1].lines);
    assert_eq!(r.cues[2].lines, orig[2].lines);
    assert_eq!((r.rejected, r.failed), (1, 1));
    assert_eq!(r.changes, vec![Change { idx: 0, before: "大家好欢迎收看".into(), after: "大家好，欢迎收看。".into() }]);
    for (a, b) in orig.iter().zip(&r.cues) {
        assert_eq!((a.start, a.end), (b.start, b.end)); // 时间永不被改
    }
}

#[test]
fn correct_prompt_mentions_scope_title_and_terms() {
    let t = term("Tauri");
    let s = correct_system(Mode::Conservative, "Rust 入门", &[&t]);
    assert!(s.contains("Do NOT paraphrase") && s.contains("Rust 入门") && s.contains("Tauri"));
    let a = correct_system(Mode::Aggressive, "", &[]);
    assert!(a.contains("grammar") && !a.contains("Video title"));
}

#[test]
fn pick_mode_prefers_request_then_punctuation_state() {
    let plain = vec![cue(0, 1000, "大家好欢迎来到本期视频今天我们来聊聊如何在家里自己做一杯好喝的咖啡首先需要准备咖啡豆")];
    let punct = vec![cue(0, 1000, "大家好，欢迎来到本期视频。今天我们来聊聊如何在家里自己做一杯好喝的咖啡，首先需要准备咖啡豆。")];
    assert_eq!(pick_mode(Some("off"), "conservative", &plain), Mode::Rules);
    assert_eq!(pick_mode(Some("aggressive"), "conservative", &punct), Mode::Aggressive);
    assert_eq!(pick_mode(None, "conservative", &plain), Mode::Conservative); // 没标点 → 用设置里的模式
    assert_eq!(pick_mode(None, "aggressive", &plain), Mode::Aggressive);
    assert_eq!(pick_mode(None, "conservative", &punct), Mode::Rules); // 有标点 → 只走规则
    assert_eq!(pick_mode(Some(""), "conservative", &plain), Mode::Conservative);
}

#[test]
fn language_guess() {
    assert_eq!(guess_lang(&[cue(0, 1, "大家好欢迎收看")]), "zh");
    assert_eq!(guess_lang(&[cue(0, 1, "Hello everyone welcome")]), "en");
    assert_eq!(guess_lang(&[cue(0, 1, "こんにちは皆さん今日は")]), "ja");
    assert_eq!(guess_lang(&[cue(0, 1, "안녕하세요 여러분")]), "ko");
}

#[tokio::test]
async fn rules_mode_correction_needs_no_model() {
    let cues = vec![cue(0, 1000, "嗯 我们用iPhone拍"), cue(1000, 2000, "第二条")];
    let r = correct_cues(None, "", &cues, Mode::Conservative, "", &[], 20, &|_, _| Ok(())).await.unwrap();
    assert_eq!(r.cues.len(), 2);
    assert_eq!(r.cues[0].lines, vec!["我们用 iPhone 拍"]);
    assert_eq!(r.changes.len(), 1);
}
