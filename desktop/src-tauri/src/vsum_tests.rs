use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Handler = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// 最小的兼容 OpenAI 的本机服务：handler 收到 user 消息，返回模型要说的话。
async fn mock(handler: Handler) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut buf = vec![];
                let mut tmp = [0u8; 8192];
                let (head_end, len) = loop {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                        let cl = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        break (p + 4, cl);
                    }
                };
                while buf.len() < head_end + len {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let v: Value = serde_json::from_slice(&buf[head_end..]).unwrap_or_default();
                let user = v.pointer("/messages/1/content").and_then(Value::as_str).unwrap_or("").to_string();
                let system = v.pointer("/messages/0/content").and_then(Value::as_str).unwrap_or("").to_string();
                let body = json!({"choices": [{"message": {"content": handler(&format!("{system}\n=====\n{user}"))}}]}).to_string();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    format!("http://{addr}")
}

fn ep(base: &str) -> Endpoint {
    Endpoint {
        base: base.to_string(),
        key: None,
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        retry_ms: 5,
        attempts: 3,
        cands: vec![],
        pool: None,
    }
}

fn req(kind: &str, n: usize) -> Req {
    Req {
        kind: kind.into(),
        lines: (0..n).map(|i| Line { t: i as u64 * 30_000, text: format!("第 {i} 句 {}", "内容".repeat(150)) }).collect(),
        ..Default::default()
    }
}

async fn run(base: &str, r: &Req) -> AppResult<Value> {
    process(&ep(base), "m", "简体中文", 20, r, &|_, _| Ok(())).await
}

#[test]
fn references_are_read_from_answers() {
    assert_eq!(extract_refs("先讲了 A [1:05]，后来 [1:05] 又提到 [1:02:03]，[x] [12]"), vec![65_000, 3_723_000]);
    assert!(extract_refs("没有引用 [a:b]").is_empty());
}

#[test]
fn outlines_keep_only_the_tree() {
    let md = "好的，这是导图：\n```markdown\n# 标题\n- 一\n  - 一·甲\n\t- 一·乙\n- 二\n\n结束语\n```";
    assert_eq!(clean_outline(md).unwrap(), "# 标题\n- 一\n  - 一·甲\n  - 一·乙\n- 二");
    assert!(clean_outline("# 只有标题\n- 唯一").is_none());
    assert!(clean_outline("随便说点什么").is_none());
}

#[test]
fn cards_quotes_and_keywords_parse_forgivingly() {
    let c = parse_cards(
        "```json\n{\"cards\":[{\"q\":\"Q1\",\"a\":\"A1\",\"time\":\"1:30\"},{\"q\":\"\",\"a\":\"x\"},{\"q\":\"Q2\",\"a\":\"A2\",\"time\":45}]}\n```",
    );
    assert_eq!(c.len(), 2);
    assert_eq!(c[0]["t"], 90_000);
    assert_eq!(c[1]["t"], 45_000);
    let q = parse_quotes(r#"{"quotes":[{"text":" 金句 ","time":"0:10"},{"text":""}]}"#);
    assert_eq!((q.len(), q[0]["text"].as_str()), (1, Some("金句")));
    assert_eq!(parse_keywords(r#"{"keywords":["a","","b"]}"#), vec!["a", "b"]);
    assert!(parse_cards("不是 JSON").is_empty());
}

#[test]
fn questions_pick_the_relevant_part_of_a_long_transcript() {
    let lines: Vec<String> = (0..400)
        .map(|i| {
            if i == 250 {
                "[8:20] 我们用火星探测器的太阳能板供电".to_string()
            } else {
                format!("[{}:00] 无关的闲聊内容 {i} {}", i / 60, "水".repeat(40))
            }
        })
        .collect();
    let ex = relevant_excerpt(&lines, "火星探测器怎么供电？", 1200);
    assert!(ex.contains("太阳能板"), "{ex}");
    assert!(ex.chars().count() <= 1400);
    let short = relevant_excerpt(&lines[..3], "任意", 10_000);
    assert_eq!(short.lines().count(), 3, "短文稿整篇都给模型");
    let hello: Vec<String> = vec!["[0:01] hello".to_string(); 3];
    let en = relevant_excerpt(&hello, "What about Mars rovers?", 5);
    assert!(en.len() <= 40);
}

#[test]
fn requests_are_validated() {
    assert!(check(&Req { kind: "nope".into(), ..Default::default() }).is_err());
    assert!(check(&req("summary", 0)).is_err(), "no lines");
    assert!(check(&req("summary", 2)).is_ok());
    assert!(check(&Req { kind: "ask".into(), lines: req("ask", 2).lines, ..Default::default() }).is_err(), "no question");
    assert!(check(&Req { kind: "rewrite".into(), text: "x".into(), format: "weird".into(), ..Default::default() }).is_err());
    assert!(check(&Req { kind: "rewrite".into(), text: "x".into(), format: "xhs".into(), ..Default::default() }).is_ok());
    assert!(check(&Req { kind: "article".into(), ..Default::default() }).is_err());
    let huge = Req { kind: "summary".into(), lines: vec![Line { t: 0, text: "字".repeat(MAX_CHARS + 1) }], ..Default::default() };
    assert!(check(&huge).is_err());
}

#[tokio::test]
async fn summary_comes_back_with_keywords_and_closed_chapters() {
    let base = mock(Arc::new(|_| {
        r#"{"summary":"概述","points":["一","二"],"keywords":["AI","视频"],"chapters":[{"time":"0:00","title":"开头"},{"time":"0:30","title":"正文"}]}"#
            .to_string()
    }))
    .await;
    let v = run(&base, &req("summary", 3)).await.unwrap();
    assert_eq!(v["summary"], "概述");
    assert_eq!(v["keywords"], json!(["AI", "视频"]));
    assert_eq!(v["chapters"][0]["endMs"], 30_000);
    assert!(v["chapters"][1]["endMs"].as_u64().unwrap() > 30_000);
}

#[tokio::test]
async fn long_videos_are_summarized_in_parts_and_merged() {
    let seen = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    let base = mock(Arc::new(move |p| {
        s2.lock().unwrap().push(p.to_string());
        if p.contains("=====\nPart 1") {
            r#"{"summary":"总摘要","points":["总要点"],"keywords":["总"]}"#.into()
        } else {
            let marker = if p.contains("(This part:)\n[") { "(This part:)\n[" } else { "=====\n[" };
            let first = p.split(marker).nth(1).and_then(|s| s.split(']').next()).unwrap_or("0:00").to_string();
            format!(r#"{{"summary":"分段","points":["p"],"keywords":["k"],"chapters":[{{"time":"{first}","title":"章 {first}"}}]}}"#)
        }
    }))
    .await;
    let v = run(&base, &req("summary", 80)).await.unwrap();
    assert!(seen.lock().unwrap().len() >= 3);
    assert_eq!((v["summary"].as_str(), v["keywords"].clone()), (Some("总摘要"), json!(["总"])));
    assert!(v["chapters"].as_array().unwrap().len() >= 2);
    // 合并这一步不再要求章节
    assert!(!seen.lock().unwrap().iter().find(|p| p.contains("=====\nPart 1")).unwrap().contains("\"chapters\""));
}

#[tokio::test]
async fn articles_have_no_chapters_and_honour_style() {
    let seen = Arc::new(Mutex::new(String::new()));
    let s2 = seen.clone();
    let base = mock(Arc::new(move |p| {
        *s2.lock().unwrap() = p.to_string();
        r#"{"summary":"文章概述","points":["a"],"keywords":[]}"#.into()
    }))
    .await;
    let r = Req { kind: "article".into(), style: "brief".into(), text: "第一段。\n\n第二段。".into(), ..Default::default() };
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["chapters"], json!([]));
    let sent = seen.lock().unwrap().clone();
    assert!(!sent.contains("\"chapters\"") && sent.contains("very short"), "{sent}");
}

#[tokio::test]
async fn mindmaps_cards_quotes_and_rewrites() {
    let base = mock(Arc::new(|p| {
        if p.contains("mind map") {
            "# 主题\n- 分支一\n  - 细节\n- 分支二".into()
        } else if p.contains("flashcards") {
            r#"{"cards":[{"q":"问","a":"答","time":"0:30"}]}"#.into()
        } else if p.contains("quotable") {
            r#"{"quotes":[{"text":"名句","time":"0:00"}]}"#.into()
        } else {
            "改写后的文案".into()
        }
    }))
    .await;
    assert_eq!(run(&base, &req("mindmap", 2)).await.unwrap()["outline"], "# 主题\n- 分支一\n  - 细节\n- 分支二");
    assert_eq!(run(&base, &req("cards", 2)).await.unwrap()["cards"][0]["t"], 30_000);
    assert_eq!(run(&base, &req("quotes", 2)).await.unwrap()["quotes"][0]["text"], "名句");
    let r = Req { kind: "rewrite".into(), format: "thread".into(), text: "摘要".into(), ..Default::default() };
    assert_eq!(run(&base, &r).await.unwrap()["text"], "改写后的文案");
    // 模型乱说话时给出可读的错误，而不是空结果
    let bad = mock(Arc::new(|_| "我不太明白".into())).await;
    assert!(run(&bad, &req("mindmap", 2)).await.is_err());
    assert!(run(&bad, &req("cards", 2)).await.is_err());
    assert!(run(&bad, &req("summary", 2)).await.is_err());
}

#[tokio::test]
async fn questions_carry_history_and_return_references() {
    let seen = Arc::new(Mutex::new(String::new()));
    let s2 = seen.clone();
    let base = mock(Arc::new(move |p| {
        *s2.lock().unwrap() = p.to_string();
        "在 [0:30] 提到了，之后 [1:00] 又解释了".into()
    }))
    .await;
    let mut r = req("ask", 4);
    r.question = "它是怎么工作的？".into();
    r.history = vec![Turn { role: "user".into(), text: "这个视频讲什么".into() }, Turn { role: "assistant".into(), text: "讲了原理".into() }];
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["refs"], json!([30_000, 60_000]));
    let sent = seen.lock().unwrap().clone();
    assert!(sent.contains("User: 这个视频讲什么") && sent.contains("Assistant: 讲了原理") && sent.contains("Question: 它是怎么工作的？"));
}

#[tokio::test]
async fn translation_keeps_timestamps() {
    let base = mock(Arc::new(|p| {
        let user = p.split("=====\n").nth(1).unwrap_or("[]");
        let src: Vec<String> = serde_json::from_str(user).unwrap_or_default();
        serde_json::to_string(&src.iter().map(|s| format!("译:{s}")).collect::<Vec<_>>()).unwrap()
    }))
    .await;
    let r = Req { kind: "translate".into(), lines: vec![Line { t: 1000, text: "hello".into() }, Line { t: 2000, text: "world".into() }], ..Default::default() };
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["lines"], json!([{"t": 1000, "text": "译:hello"}, {"t": 2000, "text": "译:world"}]));
}

#[tokio::test]
async fn cancelling_stops_the_work() {
    let base = mock(Arc::new(|_| r#"{"summary":"x","points":[],"keywords":[]}"#.into())).await;
    let out = process(&ep(&base), "m", "简体中文", 20, &req("summary", 80), &|_, _| Err(AppError::msg("已取消"))).await;
    assert_eq!(out.unwrap_err().message, "已取消");
}

#[test]
fn job_table_reports_state_and_forgets_old_jobs() {
    let (id, cancel) = new_job();
    assert_eq!(job_json(id).unwrap()["state"], "running");
    assert!(cancel_job_flag(id, &cancel));
    with_job(id, |j| j.result = Some(json!({"ok": true})));
    let v = job_json(id).unwrap();
    assert_eq!((v["state"].as_str(), v["result"]["ok"].as_bool()), (Some("done"), Some(true)));
    with_job(id, |j| j.error = Some("boom".into()));
    assert_eq!(job_json(id).unwrap()["state"], "error");
    assert!(job_json(u64::MAX).is_none());
    for _ in 0..KEEP_JOBS + 5 {
        new_job();
    }
    assert!(JOBS.lock().unwrap().len() <= KEEP_JOBS);
}

fn cancel_job_flag(id: u64, flag: &AtomicBool) -> bool {
    cancel(id) && flag.load(Ordering::Relaxed)
}

#[test]
fn transcribe_requests_need_a_web_address_and_no_transcript() {
    let ok = Req { kind: "transcribe".into(), url: "https://www.youtube.com/watch?v=abc".into(), ..Default::default() };
    assert!(check(&ok).is_ok());
    let no_url = Req { kind: "transcribe".into(), ..Default::default() };
    assert!(check(&no_url).is_err());
    let local = Req { kind: "transcribe".into(), url: "file:///etc/passwd".into(), ..Default::default() };
    assert!(check(&local).is_err());
}

#[test]
fn subtitle_requests_follow_the_same_address_rules() {
    let ok = Req { kind: "subtitles".into(), url: "https://www.bilibili.com/video/BV1".into(), ..Default::default() };
    assert!(check(&ok).is_ok());
    assert!(check(&Req { kind: "subtitles".into(), ..Default::default() }).is_err());
    assert!(check(&Req { kind: "subtitles".into(), url: "ftp://x/y".into(), ..Default::default() }).is_err());
}

#[test]
fn mindmap_outlines_keep_five_levels() {
    let out = clean_outline("# T\n- a\n  - b\n    - c\n      - d\n        - e\nnoise").unwrap();
    assert_eq!(out.lines().count(), 6);
    assert!(out.contains("        - e"));
}

#[test]
fn new_kinds_validate_their_inputs() {
    for kind in ["explain", "note", "refine"] {
        assert!(check(&Req { kind: kind.into(), ..Default::default() }).is_err(), "{kind} 需要文字");
        assert!(check(&Req { kind: kind.into(), text: "hello".into(), ..Default::default() }).is_ok());
    }
    assert!(check(&Req { kind: "punctuate".into(), ..Default::default() }).is_err());
    assert!(check(&Req { kind: "fixasr".into(), ..Default::default() }).is_err());
    assert!(check(&Req { kind: "fixasr".into(), lines: vec![Line { t: 0, text: "hello".into() }], ..Default::default() }).is_ok());
    let one = Req { kind: "punctuate".into(), lines: vec![Line { t: 0, text: "你好 世界".into() }], ..Default::default() };
    assert!(check(&one).is_ok());
    let long = Req { system: "x".repeat(20_001), ..one.clone() };
    assert!(check(&long).is_err());
}

#[test]
fn custom_prompts_replace_the_built_in_one_and_language_is_detected() {
    let mut r = Req { system: "  Answer in {lang}.  ".into(), lang: "日本語".into(), ..Default::default() };
    assert_eq!(pick_system(&r, "built-in".into()), "Answer in 日本語.");
    r.system.clear();
    assert_eq!(pick_system(&r, "built-in".into()), "built-in");
    assert!(is_chinese(&[Line { t: 0, text: "这是一段中文字幕 with a word".into() }]));
    assert!(!is_chinese(&[Line { t: 0, text: "This is English only".into() }]));
    assert!(!is_chinese(&[]));
}

#[test]
fn chunk_start_label_reads_the_first_timestamp() {
    assert_eq!(chunk_start_label("[12:30] hello\n[12:35] world"), "12:30");
    assert_eq!(chunk_start_label("\n  [1:02:03] x"), "1:02:03");
    assert_eq!(chunk_start_label("no stamps here"), "");
}

#[test]
fn overlap_adds_the_previous_tail_as_context_only() {
    let prev = "[0:01] a\n[0:02] b\n[0:03] c\n[0:04] d\n[0:05] e".to_string();
    assert_eq!(with_overlap(None, "[1:00] x"), "[1:00] x");
    let out = with_overlap(Some(&prev), "[1:00] x");
    assert!(out.contains("[0:05] e") && out.contains("[0:02] b") && !out.contains("[0:01] a"));
    assert!(out.ends_with("[1:00] x") && out.contains("do NOT summarize"));
    assert_eq!(chunk_start_ms("[1:30] hi"), 90_000);
}

#[tokio::test]
async fn a_failed_part_is_retried_then_skipped_and_reported() {
    // 含“第 40 句”的那一段模型总是胡说；其余正常。
    let calls = Arc::new(Mutex::new(0usize));
    let c2 = calls.clone();
    let base = mock(Arc::new(move |p| {
        if p.contains("=====\nPart 1") {
            return r#"{"summary":"总","points":["要点"],"keywords":["k"]}"#.into();
        }
        let part = p.rsplit("(This part:)\n").next().unwrap_or(p);
        if part.contains("第 40 句") {
            *c2.lock().unwrap() += 1;
            return "我不会 JSON".into();
        }
        r#"{"summary":"分段","points":["p"],"keywords":["k"],"chapters":[]}"#.into()
    }))
    .await;
    let v = run(&base, &req("summary", 80)).await.unwrap();
    assert_eq!(*calls.lock().unwrap(), 2, "retried once");
    assert_eq!(v["failed"].as_array().unwrap().len(), 1);
    let r = &v["failedRanges"][0];
    assert!(r[0].as_u64().unwrap() <= 40 * 30_000 && r[1].as_u64().unwrap() > 40 * 30_000);
    assert_eq!(v["summary"], "总");
}

#[tokio::test]
async fn all_parts_failing_returns_an_error() {
    let base = mock(Arc::new(|_| "不是 JSON".to_string())).await;
    assert!(run(&base, &req("summary", 3)).await.is_err());
}

#[test]
fn disk_cache_ignores_tracking_params_and_keeps_tracks_apart() {
    let dir = std::env::temp_dir().join(format!("cc-vsum-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let a = cache_file(&dir, "https://www.youtube.com/watch?v=abc&t=30s&si=xyz#frag", "sub", "en");
    let b = cache_file(&dir, "https://www.youtube.com/watch?v=abc", "sub", "en");
    assert_eq!(a, b, "分享参数、时间点不影响缓存");
    assert_ne!(b, cache_file(&dir, "https://www.youtube.com/watch?v=abc", "sub", "zh-Hans"));
    assert_ne!(b, cache_file(&dir, "https://www.youtube.com/watch?v=other", "sub", "en"));

    let url = "https://www.youtube.com/watch?v=abc";
    let lines = serde_json::json!({ "lines": [{ "t": 0, "text": "你好" }], "source": "subtitles" });
    cache_store(&dir, url, true, "en", &lines);
    assert!(cache_lookup(&dir, url, true, "en").is_some());
    assert!(cache_lookup(&dir, url, true, "zh-Hans").is_none(), "别的字幕轨不会命中");
    // 空结果不写缓存；Whisper 结果放在转写缓存里，音频转写优先命中
    cache_store(&dir, "https://x.test/v", true, "", &serde_json::json!({ "lines": [] }));
    assert!(cache_lookup(&dir, "https://x.test/v", true, "").is_none());
    cache_store(&dir, url, false, "audio", &serde_json::json!({ "lines": [{ "t": 0, "text": "转写" }], "source": "whisper" }));
    assert_eq!(cache_lookup(&dir, url, false, "audio").unwrap()["lines"][0]["text"], "转写");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn web_translation_targets_any_language_and_keeps_indexes() {
    let base = mock(Arc::new(|p| {
        // 系统提示词里要写明目标语言，且告诉模型“已经是目标语言就原样返回”
        assert!(p.contains("Japanese") && p.contains("already in"), "{p}");
        let user = p.split("=====\n").nth(1).unwrap_or("[]");
        let src: Vec<String> = serde_json::from_str(user).unwrap_or_default();
        serde_json::to_string(&src.iter().map(|s| format!("訳:{s}")).collect::<Vec<_>>()).unwrap()
    }))
    .await;
    let r = Req {
        kind: "webtrans".into(),
        lang: "Japanese".into(),
        lines: vec![Line { t: 0, text: "Hello".into() }, Line { t: 7, text: "你好".into() }],
        ..Default::default()
    };
    assert!(check(&r).is_ok());
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["lines"], json!([{"t": 0, "text": "訳:Hello"}, {"t": 7, "text": "訳:你好"}]));
    assert!(check(&Req { kind: "webtrans".into(), ..Default::default() }).is_err(), "没有文字");
}

#[tokio::test]
async fn web_translation_gets_page_domain_and_passes_it_to_the_prompt() {
    // 领域判断：返回领域标签和术语说明
    let base = mock(Arc::new(|_| {
        r#"```json
{"domain":"software development","note":"Technical tone. bug = software defect."}
```"#
            .to_string()
    }))
    .await;
    let r = Req { kind: "domain".into(), title: "Fix a bug".into(), text: "git commit merge".into(), ..Default::default() };
    assert!(check(&r).is_ok());
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["domain"], "software development");
    assert!(v["note"].as_str().unwrap().contains("bug"));
    assert!(check(&Req { kind: "domain".into(), ..Default::default() }).is_err(), "没有页面文字");
    // 翻译提示词里带上领域说明
    let base = mock(Arc::new(|p| {
        assert!(p.contains("Page context") && p.contains("bug = software defect"), "{p}");
        let user = p.split("=====\n").nth(1).unwrap_or("[]");
        let src: Vec<String> = serde_json::from_str(user).unwrap_or_default();
        serde_json::to_string(&src).unwrap()
    }))
    .await;
    let r = Req {
        kind: "webtrans".into(),
        lang: "Chinese".into(),
        context: "bug = software defect".into(),
        lines: vec![Line { t: 0, text: "a bug".into() }],
        ..Default::default()
    };
    run(&base, &r).await.unwrap();
    assert!(with_domain("S".into(), "").eq("S"));
}

#[tokio::test]
async fn comments_are_analysed_and_quotes_attached() {
    let base = mock(Arc::new(|p| {
        if p.contains("analyse the comment section") {
            r#"{"sentiment":{"pos":60,"neu":30,"neg":10},"themes":[{"name":"讲得清楚","count":2,"ids":[0,9]}],"disputes":[],"questions":[{"q":"怎么选参数","answered":false,"id":1}],"corrections":[{"text":"年份写错了","id":0}],"keywords":["入门"]}"#.into()
        } else if p.contains("verdict") {
            "整体口碑不错。".into()
        } else {
            "答案".into()
        }
    }))
    .await;
    let cs = vec![
        Cmt { author: "甲".into(), text: "05:21 讲得很清楚".into(), likes: 12, t: 321_000, replies: 0 },
        Cmt { author: "乙".into(), text: "怎么选参数？".into(), likes: 3, t: -1, replies: 1 },
    ];
    let r = Req { kind: "comments".into(), comments: cs.clone(), ..Default::default() };
    let v = run(&base, &r).await.unwrap();
    assert_eq!(v["count"], 2);
    assert_eq!(v["sentiment"]["pos"], 60);
    assert_eq!(v["verdict"], "整体口碑不错。");
    // 越界的编号被丢掉，有效编号带上原评论
    assert_eq!(v["themes"][0]["quotes"].as_array().unwrap().len(), 1);
    assert_eq!(v["themes"][0]["quotes"][0]["t"], 321_000);
    assert_eq!(v["questions"][0]["answered"], false);
    assert_eq!(v["corrections"][0]["quote"]["author"], "甲");
    let a = Req { kind: "askcomments".into(), comments: cs, question: "大家怎么看".into(), ..Default::default() };
    assert_eq!(run(&base, &a).await.unwrap()["answer"], "答案");
    assert!(check(&Req { kind: "comments".into(), ..Default::default() }).is_err());
}

#[test]
fn aligned_must_be_in_translation() {
    assert_eq!(keep_aligned("坚韧的", "这是一个坚韧的花园。"), "坚韧的");
    assert_eq!(keep_aligned("“坚韧的”。", "这是一个坚韧的花园。"), "坚韧的");
    assert_eq!(keep_aligned("顽强", "这是一个坚韧的花园。"), "");
    assert_eq!(keep_aligned("坚韧", ""), "");
    assert_eq!(keep_aligned("", "这是一个坚韧的花园。"), "");
}

#[test]
fn lookup_req_takes_translation() {
    let r: Req = serde_json::from_value(serde_json::json!({ "kind": "lookup", "text": "resilient", "trans": "坚韧的花园" })).unwrap();
    assert_eq!(r.trans, "坚韧的花园");
    let r: Req = serde_json::from_value(serde_json::json!({ "kind": "lookup", "text": "resilient" })).unwrap();
    assert!(r.trans.is_empty());
}

#[test]
fn flowgraph_is_validated_and_chunks_are_chained() {
    let g = parse_flowgraph(
        r#"```json
        {"nodes": [
          {"id": "a", "title": "发现问题", "detail": "页面很慢", "kind": "cause", "time": "0:30"},
          {"id": "b", "title": "压缩图片", "kind": "weird", "time": "2:00"},
          {"id": "b", "title": "重复"},
          {"id": "c", "title": ""},
          {"id": "d", "title": "变快", "kind": "result"}
        ],
         "edges": [
          {"from": "a", "to": "b", "label": "所以"},
          {"from": "a", "to": "b", "label": "重复"},
          {"from": "b", "to": "b"},
          {"from": "b", "to": "zzz"},
          {"from": "b", "to": "d", "label": "一个很长很长很长很长的标签"}
        ]}
        ```"#,
    )
    .unwrap();
    assert_eq!(g["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(g["nodes"][1]["kind"], "step", "不认识的类型当步骤");
    assert_eq!(g["edges"].as_array().unwrap().len(), 2);
    assert_eq!(g["edges"][1]["label"].as_str().unwrap().chars().count(), 12);
    assert!(parse_flowgraph("不是 JSON").is_none());
    assert!(parse_flowgraph(r#"{"nodes": [{"id": "a", "title": "只有一个"}], "edges": []}"#).is_none());
    // 分段的图接成一条：id 加前缀、上一段最后一个连到下一段第一个
    let j = join_flowgraphs(vec![g.clone(), g]).unwrap();
    assert_eq!(j["nodes"].as_array().unwrap().len(), 6);
    assert!(j["edges"].as_array().unwrap().iter().any(|e| e["from"] == "p0_d" && e["to"] == "p1_a" && e["label"] == "接着"));
    assert!(j["nodes"].as_array().unwrap().iter().all(|n| n["id"].as_str().unwrap().starts_with('p')));
}

#[test]
fn namer_results_are_cleaned_into_file_names() {
    let r = "```json\n[\"海边日落: 橙色的天空\", \"  a/b\\\\c?d  \", \"...\", \"x\"]\n```";
    let v = parse_names(r, 4).unwrap();
    assert_eq!(v[0], "海边日落 橙色的天空");
    assert_eq!(v[1], "a b c d");
    assert_eq!(v[2], "", "只有点号的名字留空，交给调用方退回原名");
    assert_eq!(v[3], "x");
    assert!(parse_names(r, 3).is_none(), "数量对不上就当失败");
    assert!(parse_names("没有数组", 1).is_none());
    let long = format!("[\"{}\"]", "字".repeat(100));
    assert_eq!(parse_names(&long, 1).unwrap()[0].chars().count(), 60);
    let sys = namer_system("简体中文");
    assert!(sys.contains("简体中文") && sys.contains("JSON array"));
}

#[test]
fn ocr_text_drops_fragments_and_is_capped() {
    assert_eq!(ocr_text_of(&["春季  新品", "·", "A", "SALE 50% OFF"]), "春季 新品 / SALE 50% OFF");
    assert_eq!(ocr_text_of(&[]), "");
    let long = "字".repeat(500);
    assert_eq!(ocr_text_of(&[long.as_str()]).chars().count(), 300);
}

#[test]
fn streaming_transcription_helpers() {
    use crate::subtitle::Cue;
    let cues = vec![Cue { start: 1000, end: 2000, lines: vec!["a".into()] }];
    let moved = shift_cues(cues, 90_000);
    assert_eq!((moved[0].start, moved[0].end), (91_000, 92_000));
    assert_eq!(script_lang("Hello everyone and welcome"), "");
    assert_eq!(script_lang("大家好，欢迎回来"), "zh");
    assert_eq!(script_lang("こんにちは、世界"), "ja");
    assert_eq!(script_lang("안녕하세요 여러분"), "ko");
    assert_eq!(script_lang("Windows 11 的新功能介绍"), "zh");
}

#[test]
fn a_running_job_exposes_its_partial_result_until_it_finishes() {
    let (id, _) = new_job();
    with_job(id, |j| j.partial = Some(json!({"lines": [{"t": 0, "text": "hi"}]})));
    let v = job_json(id).unwrap();
    assert_eq!(v["state"], "running");
    assert_eq!(v["partial"]["lines"][0]["text"], "hi");
    with_job(id, |j| j.result = Some(json!({"lines": []})));
    let v = job_json(id).unwrap();
    assert_eq!(v["state"], "done");
    assert!(v["partial"].is_null(), "完成后只给最终结果");
}

#[test]
fn streamed_segments_are_placed_at_their_real_start_time() {
    // 90 秒一段；split_audio 返回的起点已经是毫秒。第二段的第一句要落在 90 秒之后，不能差 1000 倍
    let cue = |a: u64, b: u64| crate::subtitle::Cue { start: a, end: b, lines: vec!["x".into()] };
    let mut all = vec![];
    append_segment(&mut all, vec![cue(0, 4_000), cue(80_000, 85_000)], 0);
    append_segment(&mut all, vec![cue(1_000, 5_000)], 90_000);
    append_segment(&mut all, vec![cue(2_000, 6_000)], 180_000);
    assert_eq!(all[2].start, 91_000);
    assert_eq!(all[3].start, 182_000);
    assert!(all.windows(2).all(|w| w[0].start <= w[1].start), "时间必须随段落单调递增");
    assert!(all.last().unwrap().end < 3 * 90_000 + 10_000);
}

// ---- 特征化：process 对每种请求类型的整体输出（返回值、进度回调序列、发给模型的提示词）。
// 拆分函数前先固定住行为；用 VSUM_GOLDEN=update 重新生成。 ----

fn short_lines(n: usize) -> Vec<Line> {
    (0..n).map(|i| Line { t: i as u64 * 65_000, text: format!(" 第{i}句 hello world ") }).collect()
}

async fn trace(r: &Req, reply: &str, cancel_at: Option<usize>) -> String {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    let reply = reply.to_string();
    let base = mock(Arc::new(move |p| {
        s2.lock().unwrap().push(p.to_string());
        reply.clone()
    }))
    .await;
    let prog: Mutex<Vec<(usize, usize)>> = Mutex::new(vec![]);
    let out = process(&ep(&base), "m", "简体中文", 20, r, &|a, b| {
        let mut g = prog.lock().unwrap();
        g.push((a, b));
        if cancel_at == Some(g.len()) {
            Err(AppError::msg("已取消"))
        } else {
            Ok(())
        }
    })
    .await;
    let mut calls = seen.lock().unwrap().clone();
    calls.sort();
    let mut s = match out {
        Ok(v) => format!("OK {v}\n"),
        Err(e) => format!("ERR {e}\n"),
    };
    s.push_str(&format!("progress {:?}\n", prog.lock().unwrap()));
    for c in calls {
        // 系统提示词很长且由各自的函数生成：只记开头、结尾和长度；用户消息超过 700 字时也只记两头和长度
        let (sys, user) = c.split_once("\n=====\n").unwrap_or((&c, ""));
        let chars: Vec<char> = sys.chars().collect();
        let head: String = chars.iter().take(60).collect();
        let tail: String = chars[chars.len().saturating_sub(160)..].iter().collect();
        let uc: Vec<char> = user.chars().collect();
        let user = if uc.len() > 700 {
            format!("({} chars) {} ... {}", uc.len(), uc[..300].iter().collect::<String>(), uc[uc.len() - 300..].iter().collect::<String>())
        } else {
            user.to_string()
        };
        s.push_str(&format!("--- system ({} chars) {head} ... {tail}\n--- user\n{user}\n", chars.len()));
    }
    s
}

fn golden_requests() -> Vec<(&'static str, Req, &'static str, Option<usize>)> {
    let l3 = || short_lines(3);
    let k = |kind: &str| Req { kind: kind.into(), ..Default::default() };
    let sm = r#"{"summary":"概述","points":["要点一","要点二"],"chapters":[{"time":"0:00","title":"开头"},{"time":"1:05","title":"中段"}],"keywords":["甲","乙"]}"#;
    let flow = r#"{"nodes":[{"id":"a","title":"起","kind":"step","time":"0:10"},{"id":"b","title":"果","kind":"result"}],"edges":[{"from":"a","to":"b","label":"导致"}]}"#;
    let w = |action: &str, cands: u8| crate::writing::Opts { action: action.into(), candidates: cands, ..Default::default() };
    vec![
        ("summary-one", Req { lines: l3(), style: "brief".into(), ..k("summary") }, sm, None),
        ("summary-many", req("summary", 80), sm, None),
        ("summary-bad-reply", Req { lines: l3(), ..k("summary") }, "不是 JSON", None),
        ("article", Req { text: "第一段\n\n  第二段  \n".into(), lang: "English".into(), ..k("article") }, sm, None),
        ("mindmap-one", Req { lines: l3(), system: "自定义 {lang}".into(), ..k("mindmap") }, "# 题\n- 一\n  - 二", None),
        ("mindmap-many", req("mindmap", 80), "# 题\n- 一\n  - 二", None),
        ("mindmap-bad", Req { lines: l3(), ..k("mindmap") }, "没有结构", None),
        ("flowgraph-one", Req { lines: l3(), ..k("flowgraph") }, flow, None),
        ("flowgraph-many", req("flowgraph", 80), flow, None),
        ("flowgraph-bad", Req { lines: l3(), ..k("flowgraph") }, "x", None),
        ("cards", Req { lines: l3(), ..k("cards") }, r#"{"cards":[{"q":"问","a":"答","time":"0:30"}]}"#, None),
        ("quotes", Req { lines: l3(), ..k("quotes") }, r#"{"quotes":[{"text":"金句","time":"0:10"}]}"#, None),
        ("cards-empty", Req { lines: l3(), ..k("cards") }, "{}", None),
        (
            "ask",
            Req {
                lines: l3(),
                question: " 讲了什么？ ".into(),
                history: vec![Turn { role: "user".into(), text: "之前问".into() }, Turn { role: "assistant".into(), text: " ".into() }, Turn { role: "assistant".into(), text: "之前答".into() }],
                ..k("ask")
            },
            " 答案见 [1:05] ",
            None,
        ),
        ("ask-cancel", Req { lines: l3(), question: "q".into(), ..k("ask") }, "a", Some(1)),
        (
            "askcomments",
            Req { question: "大家怎么看".into(), comments: vec![Cmt { author: "甲".into(), text: "好看".into(), likes: 5, t: -1, replies: 0 }, Cmt { author: "乙".into(), text: "一般".into(), likes: 1, t: 3000, replies: 2 }], ..k("askcomments") },
            " 多数好评 ",
            None,
        ),
        ("translate-zh", Req { lines: vec![Line { t: 0, text: "你好".into() }, Line { t: 1000, text: " ".into() }, Line { t: 2000, text: "再见".into() }], guide: "\nGuide".into(), ..k("translate") }, r#"["Hello","Bye"]"#, None),
        ("translate-en", Req { lines: l3(), lang: "日本語".into(), ..k("translate") }, r#"["一","二","三"]"#, None),
        ("webtrans", Req { lines: l3(), context: "站点：x".into(), guide: "\nG".into(), ..k("webtrans") }, r#"["一","二","三"]"#, None),
        ("punctuate", Req { lines: l3(), title: " 标题 ".into(), ..k("punctuate") }, r#"["一。","二。","三。"]"#, None),
        ("fixasr", Req { lines: l3(), title: "T".into(), system: "own".into(), ..k("fixasr") }, r#"["一","二"]"#, None),
        ("namer", Req { lines: l3(), title: "页面".into(), text: " a  b\nc ".into(), lang: "English".into(), ..k("namer") }, r#"["甲 乙","b/c","d."]"#, None),
        ("namer-bad", Req { lines: l3(), ..k("namer") }, "没有数组", None),
        ("domain", Req { title: "T".into(), url: " http://x ".into(), text: " 样本 ".into(), ..k("domain") }, r#"{"domain":"科技","note":"说明"}"#, None),
        ("domain-bad", k("domain"), "不是 JSON", None),
        ("answer", Req { title: "T".into(), text: " 选中 ".into(), ..k("answer") }, " 回答 ", None),
        ("answer-summarize", Req { title: "T".into(), text: "选中".into(), context: "前后文".into(), format: "summarize".into(), ..k("answer") }, "摘", None),
        ("explain", Req { title: "T".into(), text: "词".into(), context: " ".into(), ..k("explain") }, " 解释 ", None),
        ("lookup", Req { title: "T".into(), text: " hello ".into(), context: "Hello world".into(), trans: " 你好世界 ".into(), ..k("lookup") }, r#"{"meaning":"问候","aligned":"你好"}"#, None),
        ("lookup-bad", Req { text: "x".into(), ..k("lookup") }, "不是 JSON", None),
        ("note", Req { title: "T".into(), text: "目标".into(), context: "全文".into(), ..k("note") }, r#"{"quote":" 笔记 "}"#, None),
        ("note-plain", Req { title: "T".into(), text: "目标".into(), ..k("note") }, "quote: \"纯文本笔记\"", None),
        ("note-empty", Req { ..k("note") }, "  ", None),
        ("refine", Req { title: "T".into(), text: "原笔记".into(), ..k("refine") }, r#"{"quote":"改后"}"#, None),
        ("rewrite", Req { text: " 原文 ".into(), format: "xhs".into(), ..k("rewrite") }, " 改写 ", None),
        ("replyread", Req { text: "对方的话".into(), write: w("reply", 1), ..k("replyread") }, r#"{"ask":"问价。","facts":"事实","relation":"客户"}"#, None),
        ("write-reply-3", Req { text: "收到的消息 12:30".into(), write: w("reply", 3), ..k("write") }, " 好的，12:30 见 ", None),
        ("write-reply-1", Req { text: "消息".into(), write: w("reply", 1), ..k("write") }, "OK", None),
        ("write-tweak", Req { text: "草稿".into(), write: w("tweak", 1), ..k("write") }, " 改后 ", None),
        ("write-tweak-empty", Req { text: "草稿".into(), write: w("tweak", 1), ..k("write") }, " ", None),
        ("write-default-2", Req { text: "草稿 3 个".into(), write: w("polish", 2), ..k("write") }, "同一版", None),
        ("write-persona", Req { text: "草稿".into(), write: crate::writing::Opts { extra: "人设 9".into(), ..w("persona", 1) }, ..k("write") }, "写了 100 个", None),
        ("write-empty", Req { text: "草稿".into(), write: w("polish", 1), ..k("write") }, "", None),
        ("style", Req { text: "我写的一段话。".into(), ..k("style") }, "简洁，爱用短句", None),
        ("style-empty", Req { text: "x".into(), ..k("style") }, "", None),
        ("grammar-ok", Req { text: "他们是好人 .".into(), ..k("grammar") }, r#"{"issues":[{"from":"他们是好人 .","to":"他们是好人。","why":"标点"}]}"#, None),
        ("grammar-model-junk", Req { text: "ok  text".into(), ..k("grammar") }, "不是 JSON", None),
        ("stats-empty-plan", Req { text: "一段文字".into(), ..k("stats") }, r#"{"queries":[]}"#, None),
        ("unsupported", k("nope"), "x", None),
        ("empty-lang-default", Req { lines: l3(), lang: "  ".into(), ..k("namer") }, r#"["a","b","c"]"#, None),
    ]
}

#[tokio::test]
async fn process_output_matches_the_recorded_snapshots() {
    let mut got = String::new();
    for (name, r, reply, cancel) in golden_requests() {
        got.push_str(&format!("=== {name}\n{}\n", trace(&r, reply, cancel).await));
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/vsum_golden.txt");
    if std::env::var("VSUM_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(path, &got).unwrap();
    }
    assert_eq!(got, include_str!("vsum_golden.txt"), "process 的输出变了：确认是有意的改动后，用 VSUM_GOLDEN=update 重新生成");
}
