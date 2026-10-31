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
    Endpoint { base: base.to_string(), key: None, client: reqwest::Client::builder().no_proxy().build().unwrap(), retry_ms: 5 }
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
