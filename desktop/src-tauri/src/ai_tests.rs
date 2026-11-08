use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Handler = Arc<dyn Fn(&str, &[u8], usize) -> (u16, String) + Send + Sync>;

/// 一个最小的本机 HTTP 服务，模拟兼容 OpenAI 的接口。handler 参数：路径、请求体、第几次请求。
async fn mock(handler: Handler) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(Mutex::new(0usize));
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = vec![];
                let mut tmp = [0u8; 8192];
                let (head_end, content_len) = loop {
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
                while buf.len() < head_end + content_len {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let n = {
                    let mut c = count.lock().unwrap();
                    *c += 1;
                    *c
                };
                let (status, body) = handler(&path, &buf[head_end..], n);
                let resp =
                    format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
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
        key: Some("sk-test".into()),
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        retry_ms: 5,
        attempts: 3,
        cands: vec![],
        pool: None,
    }
}

fn reply(content: &str) -> String {
    json!({"choices": [{"message": {"role": "assistant", "content": content}}]}).to_string()
}

fn cue(s: u64, t: &str) -> Cue {
    Cue { start: s, end: s + 1000, lines: vec![t.into()] }
}

/// 从请求体里取出用户消息里的 JSON 数组。
fn user_array(body: &[u8]) -> Vec<String> {
    let v: Value = serde_json::from_slice(body).unwrap();
    let user = v.pointer("/messages/1/content").and_then(Value::as_str).unwrap();
    serde_json::from_str(user).unwrap()
}

#[test]
fn array_parsing_is_forgiving() {
    assert_eq!(parse_array(r#"["a","b"]"#, 2).unwrap(), vec!["a", "b"]);
    assert_eq!(parse_array("```json\n[\"你好\", \"再见\"]\n```", 2).unwrap(), vec!["你好", "再见"]);
    assert_eq!(parse_array("Here you go:\n[\"x\"]\nHope it helps", 1).unwrap(), vec!["x"]);
    assert_eq!(parse_array(r#"{"translations": ["a", "b"]}"#, 2).unwrap(), vec!["a", "b"]);
    assert_eq!(parse_array("[1, \"b\"]", 2).unwrap(), vec!["1", "b"]);
    assert!(parse_array(r#"["a"]"#, 2).is_none(), "wrong count");
    assert!(parse_array("sorry, I cannot", 1).is_none());
}

#[tokio::test]
async fn translation_batches_and_bilingual_output() {
    let seen = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    let base = mock(Arc::new(move |path, body, _| {
        assert_eq!(path, "/chat/completions");
        let arr = user_array(body);
        s2.lock().unwrap().push(arr.len());
        let out: Vec<String> = arr.iter().map(|t| format!("译:{t}")).collect();
        (200, reply(&serde_json::to_string(&out).unwrap()))
    }))
    .await;
    let cues: Vec<Cue> = (0..5).map(|i| cue(i * 1000, &format!("line{i}"))).collect();
    let progress = |_: usize, _: usize| Ok(());
    let t = translate_cues(&ep(&base), "m", "简体中文", &cues, 2, &progress).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![2, 2, 1]);
    assert_eq!(t.failed, 0);
    let bi = apply_translation(&cues, &t.texts, true);
    assert_eq!(bi[0].lines, vec!["line0", "译:line0"]);
    let only = apply_translation(&cues, &t.texts, false);
    assert_eq!(only[4].lines, vec!["译:line4"]);
}

#[tokio::test]
async fn mismatched_batches_are_split_until_they_work() {
    let base = mock(Arc::new(|_, body, _| {
        let arr = user_array(body);
        // 模型只会处理 1 条一次：多条时漏掉一条
        let out: Vec<String> =
            if arr.len() > 1 { arr[..arr.len() - 1].iter().map(|t| format!("T{t}")).collect() } else { arr.iter().map(|t| format!("T{t}")).collect() };
        (200, reply(&serde_json::to_string(&out).unwrap()))
    }))
    .await;
    let cues: Vec<Cue> = (0..4).map(|i| cue(i * 1000, &i.to_string())).collect();
    let t = translate_cues(&ep(&base), "m", "English", &cues, 4, &|_, _| Ok(())).await.unwrap();
    assert_eq!(t.failed, 0);
    assert_eq!(t.texts.iter().map(|x| x.clone().unwrap()).collect::<Vec<_>>(), vec!["T0", "T1", "T2", "T3"]);
}

#[tokio::test]
async fn a_hopeless_line_keeps_its_original_text() {
    let base = mock(Arc::new(|_, body, _| {
        let arr = user_array(body);
        if arr.iter().any(|t| t == "bad") {
            (200, reply("I cannot translate this"))
        } else {
            (200, reply(&serde_json::to_string(&arr.iter().map(|t| format!("T{t}")).collect::<Vec<_>>()).unwrap()))
        }
    }))
    .await;
    let cues = vec![cue(0, "a"), cue(1000, "bad"), cue(2000, "c")];
    let t = translate_cues(&ep(&base), "m", "English", &cues, 3, &|_, _| Ok(())).await.unwrap();
    assert_eq!(t.failed, 1);
    let out = apply_translation(&cues, &t.texts, false);
    assert_eq!(out[0].lines, vec!["Ta"]);
    assert_eq!(out[1].lines, vec!["bad"], "original kept");
    assert_eq!(out[2].lines, vec!["Tc"]);
}

#[tokio::test]
async fn rate_limits_are_retried_and_bad_keys_reported() {
    let base = mock(Arc::new(|_, _, n| if n < 3 { (429, r#"{"error":{"message":"slow down"}}"#.into()) } else { (200, reply("[\"ok\"]")) })).await;
    let r = chat(&ep(&base), "m", "s", "[\"x\"]").await.unwrap();
    assert_eq!(r, "[\"ok\"]");
    let base = mock(Arc::new(|_, _, _| (401, r#"{"error":{"message":"Incorrect API key provided"}}"#.into()))).await;
    let e = chat(&ep(&base), "m", "s", "u").await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::NeedLogin);
    assert!(e.message.contains("密钥") && e.message.contains("Incorrect API key"), "{}", e.message);
    // 一直 429：重试 3 次后放弃
    let base = mock(Arc::new(|_, _, _| (429, "{}".into()))).await;
    assert_eq!(chat(&ep(&base), "m", "s", "u").await.unwrap_err().kind, ErrorKind::RateLimited);
    // 不是 JSON
    let base = mock(Arc::new(|_, _, _| (200, "<html>hi</html>".into()))).await;
    assert!(chat(&ep(&base), "m", "s", "u").await.unwrap_err().message.contains("兼容 OpenAI"));
    // 连不上
    assert_eq!(chat(&ep("http://127.0.0.1:1"), "m", "s", "u").await.unwrap_err().kind, ErrorKind::Network);
}

#[tokio::test]
async fn cancellation_stops_translation() {
    let base = mock(Arc::new(|_, body, _| (200, reply(&serde_json::to_string(&user_array(body)).unwrap())))).await;
    let cues: Vec<Cue> = (0..10).map(|i| cue(i * 1000, "x")).collect();
    let calls = Arc::new(Mutex::new(0));
    let c2 = calls.clone();
    let progress = move |_: usize, _: usize| {
        let mut c = c2.lock().unwrap();
        *c += 1;
        if *c >= 3 {
            Err(AppError::msg("canceled"))
        } else {
            Ok(())
        }
    };
    let e = translate_cues(&ep(&base), "m", "x", &cues, 2, &progress).await.map(|_| ()).unwrap_err();
    assert_eq!(e.message, "canceled");
}

#[test]
fn language_helpers() {
    assert_eq!(lang_tag("简体中文"), "zh");
    assert_eq!(lang_tag("繁體中文"), "zh-Hant");
    assert_eq!(lang_tag("English"), "en");
    assert_eq!(lang_tag("日本語"), "ja");
    assert_eq!(lang_tag("Klingon"), "ai");
    for (name, tag) in
        [("Português", "pt"), ("Bahasa Indonesia", "id"), ("Tiếng Việt", "vi"), ("日语", "ja"), ("Türkçe", "tr"), ("українська", "uk"), ("العربية", "ar")]
    {
        assert_eq!(lang_tag(name), tag, "{name}");
    }
    assert_eq!(strip_lang_suffix("课程.en"), "课程");
    assert_eq!(strip_lang_suffix("课程.zh-Hant"), "课程");
    assert_eq!(strip_lang_suffix("课程"), "课程");
    assert_eq!(strip_lang_suffix("v1.0 的发布"), "v1.0 的发布", "not a language tag");
    assert_eq!(strip_lang_suffix(".hidden"), ".hidden");
}

#[test]
fn summary_parsing_and_rendering() {
    let resp = "```json\n{\"summary\": \"讲了覆盖率。\", \"points\": [\"一\", \" 二 \", \"\"], \"chapters\": [{\"time\": \"1:05\", \"title\": \"正文\"}, {\"time\": \"0:00\", \"title\": \"开场\"}, {\"time\": \"01:02:03\", \"title\": \"结尾\"}, {\"time\": \"bad\", \"title\": \"x\"}, {\"time\": 90, \"title\": \"数字时间\"}]}\n```";
    let s = parse_summary(resp).unwrap();
    assert_eq!(s.summary, "讲了覆盖率。");
    assert_eq!(s.points, vec!["一", "二"]);
    let spans: Vec<(u64, &str)> = s.chapters.iter().map(|c| (c.start_ms, c.title.as_str())).collect();
    assert_eq!(spans, vec![(0, "开场"), (65_000, "正文"), (90_000, "数字时间"), (3_723_000, "结尾")]);
    let closed = close_chapters(s.chapters.clone(), 4_000_000);
    assert_eq!((closed[0].end_ms, closed[3].end_ms), (65_000, 4_000_000));
    assert_eq!(chapters_text(&s.chapters).lines().next().unwrap(), "0:00 开场");
    assert!(chapters_text(&s.chapters).contains("1:02:03 结尾"));
    let md = summary_markdown("课程", &s);
    assert!(md.starts_with("# 课程\n") && md.contains("## 摘要") && md.contains("- 一") && md.contains("- 0:00 开场"), "{md}");
    assert!(parse_summary("no json here").is_none());
    assert!(parse_summary("{}").is_none());
    assert_eq!(parse_label("[1:23]"), Some(83_000));
    assert_eq!(parse_label("83"), Some(83_000));
    assert_eq!(parse_label("a:b"), None);
}

#[test]
fn transcripts_are_chunked_on_line_boundaries() {
    let cues: Vec<Cue> = (0..30).map(|i| cue(i * 61_000, &"字".repeat(40))).collect();
    let lines = transcript_lines(&cues);
    assert!(lines[1].starts_with("[1:01] "));
    let chunks = chunk_lines(&lines, 300);
    assert!(chunks.len() > 3);
    assert!(chunks.iter().all(|c| c.chars().count() <= 300 || !c.contains('\n')));
    assert_eq!(chunks.join("\n").lines().count(), 30, "no line is lost or split");
    assert!(chunk_lines(&[], 100).is_empty());
}

#[tokio::test]
async fn long_transcripts_are_summarized_in_parts_and_merged() {
    let calls = Arc::new(Mutex::new(vec![]));
    let c2 = calls.clone();
    let base = mock(Arc::new(move |_, body, _| {
        let v: Value = serde_json::from_slice(body).unwrap();
        let user = v.pointer("/messages/1/content").and_then(Value::as_str).unwrap().to_string();
        c2.lock().unwrap().push(user.clone());
        if user.starts_with("Part 1") {
            (200, reply(r#"{"summary":"总的摘要","points":["总要点"]}"#))
        } else {
            let first = user.lines().next().unwrap_or("").trim_start_matches('[').split(']').next().unwrap_or("0:00").to_string();
            (200, reply(&format!(r#"{{"summary":"分段摘要","points":["p"],"chapters":[{{"time":"{first}","title":"章 {first}"}}]}}"#)))
        }
    }))
    .await;
    let cues: Vec<Cue> = (0..60).map(|i| cue(i * 30_000, &"内容".repeat(150))).collect();
    let s = summarize(&ep(&base), "m", "简体中文", &cues, &|_, _| Ok(())).await.unwrap();
    let n = calls.lock().unwrap().len();
    assert!(n >= 3, "several chunks plus the merge request: {n}");
    assert_eq!((s.summary.as_str(), s.points.clone()), ("总的摘要", vec!["总要点".to_string()]));
    assert!(s.chapters.len() >= 2 && s.chapters[0].start_ms == 0, "{:?}", s.chapters);
    assert!(s.chapters.windows(2).all(|w| w[0].start_ms < w[1].start_ms));
}

#[test]
fn multipart_body_shape() {
    let body = multipart(&[("model", "whisper-1"), ("language", "zh")], "file", "a.m4a", "audio/mp4", b"BIN", "XX");
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(text.starts_with("--XX\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n"), "{text}");
    assert!(text.contains("name=\"file\"; filename=\"a.m4a\"\r\nContent-Type: audio/mp4\r\n\r\nBIN\r\n--XX--\r\n"));
}

#[test]
fn whisper_cli_helpers() {
    let a = whisper_args("/m/ggml-base.bin", Path::new("/t/a.wav"), Path::new("/t/out"), "");
    assert_eq!(&a[..6], ["-m", "/m/ggml-base.bin", "-f", "/t/a.wav", "-l", "auto"]);
    assert!(a.contains(&"-osrt".to_string()));
    assert_eq!(whisper_args("m", Path::new("a"), Path::new("o"), "zh")[5], "zh");
    assert_eq!(whisper_progress("whisper_print_progress_callback: progress =  35%"), Some(35.0));
    assert_eq!(whisper_progress("something else"), None);
}

fn ffmpeg_ok() -> bool {
    std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[tokio::test]
async fn audio_is_split_and_transcribed_with_offsets() {
    if !ffmpeg_ok() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("clearclip-stt-test-{}-{}", std::process::id(), crate::db::now()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("long.mp4");
    let mut a = base_args();
    a.extend(
        ["-f", "lavfi", "-i", "sine=frequency=300:duration=25", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=5:duration=25", "-c:a", "aac", "-c:v", "mpeg4"]
            .map(String::from),
    );
    a.push(src.to_string_lossy().into_owned());
    run_ffmpeg_in(Path::new("ffmpeg"), &a, None).await.unwrap();
    let parts = split_audio(Path::new("ffmpeg"), &src, &dir.join("parts"), 10).await.unwrap();
    assert_eq!(parts.len(), 3, "{parts:?}");
    assert_eq!(parts.iter().map(|p| p.1).collect::<Vec<_>>(), vec![0, 10_000, 20_000]);
    assert!(parts.iter().all(|(p, _)| std::fs::metadata(p).unwrap().len() > 500));

    let got = Arc::new(Mutex::new(vec![]));
    let g2 = got.clone();
    let base = mock(Arc::new(move |path, body, n| {
        assert_eq!(path, "/audio/transcriptions");
        let text = String::from_utf8_lossy(body).into_owned();
        assert!(
            text.contains("name=\"model\"\r\n\r\nwhisper-1")
                && text.contains("name=\"response_format\"\r\n\r\nsrt")
                && text.contains("name=\"language\"\r\n\r\nzh")
        );
        assert!(text.contains("filename=\"part_00"));
        g2.lock().unwrap().push(n);
        // 接口返回 SRT 纯文本（不是 JSON）
        (200, format!("1\n00:00:01,000 --> 00:00:02,000\n第{n}段\n\n"))
    }))
    .await;
    let e = ep(&base);
    let mut texts = vec![];
    for (file, offset) in &parts {
        texts.push((transcribe_chunk(&e, "whisper-1", "zh", file).await.unwrap(), *offset));
    }
    let cues = merge_srt_parts(texts);
    let starts: Vec<u64> = cues.iter().map(|c| c.start).collect();
    assert_eq!(starts, vec![1000, 11_000, 21_000]);
    assert_eq!(got.lock().unwrap().len(), 3);
    // 接口返回的不是字幕
    let bad = mock(Arc::new(|_, _, _| (200, r#"{"text":"hello"}"#.into()))).await;
    assert!(transcribe_chunk(&ep(&bad), "whisper-1", "", &parts[0].0).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cloud_stt_rotates_to_next_service_when_one_fails() {
    let dir = std::env::temp_dir().join(format!("clearclip-stt-rot-{}-{}", std::process::id(), crate::db::now()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.m4a");
    std::fs::write(&file, b"fake audio").unwrap();
    let limited = mock(Arc::new(|_, _, _| (429, r#"{"error":{"message":"quota"}}"#.into()))).await;
    let good = mock(Arc::new(|_, body, _| {
        let t = String::from_utf8_lossy(body).into_owned();
        // 用的是第二家自己的语音模型，不是第一家的
        assert!(t.contains("name=\"model\"\r\n\r\nmodel-b"), "{t}");
        (200, "1\n00:00:01,000 --> 00:00:02,000\n好\n\n".into())
    }))
    .await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let cand = |id: &str, base: &str, model: &str| Candidate {
        id: id.into(),
        name: id.into(),
        base: base.into(),
        key: Some("k".into()),
        model: model.into(),
        client: client.clone(),
        kind: crate::mt::Kind::Chat,
    };
    let pool = Arc::new(crate::ai_pool::AiPool::default());
    let mut e = ep(&limited);
    e.attempts = 1;
    e.cands = vec![cand("a", &limited, "model-a"), cand("b", &good, "model-b")];
    e.pool = Some(pool.clone());
    let out = transcribe_rotating(&e, "ignored", "zh", &file).await.unwrap();
    assert!(out.contains("-->"));
    // 第一家被记下冷却，下次排队时会被跳过
    assert!(pool.snapshot(crate::db::now()).get("a").is_some_and(|r| r.cool_until > crate::db::now()));
    // 都失败时报出每一家
    let mut e2 = ep(&limited);
    e2.attempts = 1;
    e2.cands = vec![cand("a", &limited, "m"), cand("c", &limited, "m")];
    let err = transcribe_rotating(&e2, "x", "", &file).await.unwrap_err();
    assert!(err.message.contains("2 个语音转文字服务都失败"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn reflection_improves_good_drafts_and_keeps_bad_ones() {
    let base = mock(Arc::new(|path, body, _| {
        assert_eq!(path, "/chat/completions");
        let req: Value = serde_json::from_slice(body).unwrap();
        let user = req["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("SOURCE: ") && user.contains("DRAFT: "), "{user}");
        // 第 1 条润色；第 2 条把数字改了（应被拒绝）；第 3 条原样返回
        (200, reply(r#"["你好，世界！","我们有 9 个方案","DRAFT: 再见"]"#))
    }))
    .await;
    let cues = vec![cue(0, "hello world"), cue(1000, "we have 3 plans"), cue(2000, "bye")];
    let drafts = vec![Some("你好 世界".to_string()), Some("我们有 3 个方案".to_string()), Some("再见".to_string())];
    let noop = |_: usize, _: usize| Ok(());
    let r = reflect_translations(&ep(&base), "m", "简体中文", &cues, &drafts, 10, &[], &noop).await.unwrap();
    assert_eq!(r.texts[0].as_deref(), Some("你好，世界！"));
    assert_eq!(r.texts[1].as_deref(), Some("我们有 3 个方案"));
    assert_eq!(r.texts[2].as_deref(), Some("再见"));
    assert_eq!((r.changed, r.kept), (1, 2));
}

#[test]
fn settings_are_normalized() {
    let mut s = crate::settings::AiSettings {
        base_url: " https://api.x.com/v1/ ".into(),
        stt_base_url: "ftp://nope".into(),
        batch_size: 1,
        stt_engine: "weird".into(),
        target_lang: "  ".into(),
        ..Default::default()
    };
    s.normalize();
    assert_eq!(s.base_url, "https://api.x.com/v1");
    assert_eq!(s.stt_base_url, "");
    assert_eq!(s.stt_url(), "https://api.x.com/v1", "falls back to the chat endpoint");
    assert_eq!((s.batch_size, s.stt_engine.as_str(), s.target_lang.as_str()), (5, "local", "简体中文"));
}

#[test]
fn existing_subtitles_decide_auto_translation() {
    assert!(has_lang(&["sub-en".into(), "sub-zh-Hans".into()], "zh"));
    assert!(has_lang(&["sub-ai-zh-ab12cd".into()], "zh"), "an earlier AI translation counts");
    assert!(has_lang(&["cc-ai-zh".into()], "zh"));
    assert!(!has_lang(&["sub-en".into(), "danmaku".into(), "auto-en".into()], "zh"));
    assert!(!has_lang(&["sub-zh".into()], "ai"), "unknown target language never counts as present");
    assert!(is_local_url("http://localhost:11434/v1") && is_local_url("http://127.0.0.1:8080") && !is_local_url("https://api.openai.com/v1"));
}

// ---------- 模型池 ----------

fn cand_for(id: &str, base: &str, model: &str) -> Candidate {
    Candidate {
        id: id.into(),
        name: id.into(),
        base: base.into(),
        key: Some("k".into()),
        model: model.into(),
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        kind: crate::mt::Kind::Chat,
    }
}

fn pooled(cands: Vec<Candidate>, pool: Arc<AiPool>) -> Endpoint {
    Endpoint { base: cands[0].base.clone(), key: None, client: reqwest::Client::new(), retry_ms: 5, attempts: 1, cands, pool: Some(pool) }
}

#[tokio::test]
async fn pool_fails_over_when_the_first_service_is_out_of_quota() {
    let seen_models = Arc::new(Mutex::new(Vec::<String>::new()));
    let a = mock(Arc::new(|_, _, _| (429, r#"{"error":{"message":"You exceeded your current quota"}}"#.into()))).await;
    let sm = seen_models.clone();
    let b = mock(Arc::new(move |_, body, _| {
        let v: Value = serde_json::from_slice(body).unwrap();
        sm.lock().unwrap().push(v["model"].as_str().unwrap().to_string());
        (200, reply("OK-B"))
    }))
    .await;
    let pool = Arc::new(AiPool::default());
    let ep = pooled(vec![cand_for("a", &a, "model-a"), cand_for("b", &b, "model-b")], pool.clone());
    let out = chat(&ep, "ignored", "s", "u").await.unwrap();
    assert_eq!(out, "OK-B");
    assert_eq!(*seen_models.lock().unwrap(), vec!["model-b"], "用的是第二家自己的模型，不是调用方传的");
    let snap = pool.snapshot(crate::db::now());
    assert!(snap["a"].cool_until > crate::db::now(), "额度用完 → 冷却到次日零点（临近零点时可能不足一小时）");
    assert_eq!(snap["b"].ok, 1);
}

#[tokio::test]
async fn pool_reports_every_service_when_all_fail_and_keeps_the_error_kind() {
    let a = mock(Arc::new(|_, _, _| (401, r#"{"error":{"message":"bad key"}}"#.into()))).await;
    let b = mock(Arc::new(|_, _, _| (500, "oops".into()))).await;
    let pool = Arc::new(AiPool::default());
    let ep = pooled(vec![cand_for("a", &a, "m"), cand_for("b", &b, "m")], pool.clone());
    let err = chat(&ep, "", "s", "u").await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Network, "保留最后一个错误的分类");
    assert!(err.message.contains("2 个服务都失败") && err.message.contains("a：") && err.message.contains("b："), "{}", err.message);
    assert!(pool.snapshot(crate::db::now())["a"].bad_key);
}

#[tokio::test]
async fn single_pool_service_keeps_its_own_error_message() {
    let a = mock(Arc::new(|_, _, _| (404, r#"{"error":{"message":"no such model"}}"#.into()))).await;
    let ep = pooled(vec![cand_for("a", &a, "m")], Arc::new(AiPool::default()));
    let err = chat(&ep, "", "s", "u").await.unwrap_err();
    assert!(err.message.contains("no such model") && !err.message.contains("个服务"), "{}", err.message);
}

#[tokio::test]
async fn glossary_terms_reach_only_the_batches_that_mention_them() {
    let systems = Arc::new(Mutex::new(vec![]));
    let s2 = systems.clone();
    let base = mock(Arc::new(move |_, body, _| {
        let v: serde_json::Value = serde_json::from_slice(body).unwrap();
        s2.lock().unwrap().push(v["messages"][0]["content"].as_str().unwrap().to_string());
        let arr = user_array(body);
        (200, reply(&serde_json::to_string(&arr.iter().map(|t| format!("T{t}")).collect::<Vec<_>>()).unwrap()))
    }))
    .await;
    let cues = vec![cue(0, "The Kubernetes pod restarted"), cue(1000, "nothing special"), cue(2000, "pod again")];
    let terms = vec![
        crate::glossary::Term { source: "pod".into(), target: "容器组".into(), note: String::new(), ..Default::default() },
        crate::glossary::Term { source: "Helm".into(), target: "Helm".into(), note: String::new(), ..Default::default() },
    ];
    let t = translate_cues_g(&ep(&base), "m", "简体中文", &cues, 2, &terms, &|_, _| Ok(())).await.unwrap();
    assert_eq!(t.failed, 0);
    let sys = systems.lock().unwrap().clone();
    assert_eq!(sys.len(), 2);
    assert!(sys[0].contains("- pod => 容器组"), "第一批提到了 pod");
    assert!(!sys[0].contains("Helm"), "没出现的术语不放进提示词");
    assert!(sys[1].contains("- pod => 容器组"), "第二批也提到了 pod");
    let none = translate_cues_g(&ep(&base), "m", "简体中文", &cues[1..2], 5, &terms, &|_, _| Ok(())).await.unwrap();
    assert_eq!(none.failed, 0);
    assert!(!systems.lock().unwrap().last().unwrap().contains("Glossary"), "没有命中时提示词保持原样");
}

#[tokio::test]
async fn subgen_correction_goes_through_guard_with_a_model() {
    use crate::subgen::{correct_cues, Mode};
    // 模型：第 1 行补标点（采用）；第 2 行整句重写（拒绝）；第 3 行改了数字（拒绝）；第 4 行改同音字（采用）
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    let base = mock(Arc::new(move |_p, body, _n| {
        s2.lock().unwrap().push(String::from_utf8_lossy(body).into_owned());
        (200, reply(r#"["大家好，欢迎收看。","这是一段被模型彻底改写成别的意思的长句子","一共有36个人","这是每个人的权力。"]"#))
    }))
    .await;
    let cues = vec![cue(0, "大家好欢迎收看"), cue(1000, "今天讲咖啡"), cue(2000, "一共有35个人"), cue(3000, "这是每个人的权利")];
    let r = correct_cues(Some(&ep(&base)), "m", &cues, Mode::Conservative, "咖啡入门", &[], 20, &|_, _| Ok(())).await.unwrap();
    assert_eq!(r.cues[0].lines, vec!["大家好，欢迎收看。"]);
    assert_eq!(r.cues[1].lines, cues[1].lines);
    assert_eq!(r.cues[2].lines, vec!["一共有 35 个人"]); // 模型改的数字被拒绝，只保留规则层加的空格
    assert_eq!(r.rejected, 2);
    assert_eq!(r.changes.len(), 3); // 第 1、4 行是模型改的，第 3 行是规则层加的空格
    assert!(r.cues.iter().zip(&cues).all(|(a, b)| a.start == b.start && a.end == b.end));
    // 提示词里带了视频标题
    assert!(seen.lock().unwrap()[0].contains("咖啡入门"));
}

#[tokio::test]
async fn subgen_correction_wrong_line_count_keeps_original() {
    use crate::subgen::{correct_cues, Mode};
    // 模型总是少回一行：整批无法对齐，最终逐条保留原文，不会错位
    let base = mock(Arc::new(|_p, _b, _n| (200, reply(r#"["只有一行"]"#)))).await;
    let cues = vec![cue(0, "第一句话"), cue(1000, "第二句话")];
    let r = correct_cues(Some(&ep(&base)), "m", &cues, Mode::Conservative, "", &[], 20, &|_, _| Ok(())).await.unwrap();
    assert_eq!(r.cues.len(), 2);
    assert_eq!(r.cues[1].lines, cues[1].lines);
}

// ---------- 机器翻译服务（Google / 微软 / DeepL / DeepLX）接进模型池 ----------

fn mt_cand(id: &str, kind: crate::mt::Kind, base: &str, key: Option<&str>, region: &str) -> Candidate {
    Candidate {
        id: id.into(),
        name: id.into(),
        base: base.into(),
        key: key.map(str::to_string),
        model: region.into(),
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        kind,
    }
}

#[tokio::test]
async fn each_machine_translation_provider_speaks_its_own_protocol() {
    use crate::mt::{translate, Kind};
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let texts = vec!["Hello".to_string(), "It's fine".to_string()];

    // Google：key 走查询参数，目标 zh-CN，返回里的 HTML 实体要还原
    let seen = Arc::new(Mutex::new(String::new()));
    let s = seen.clone();
    let g = mock(Arc::new(move |path, body, _| {
        *s.lock().unwrap() = format!("{path} {}", String::from_utf8_lossy(body));
        (200, r#"{"data":{"translations":[{"translatedText":"你好"},{"translatedText":"It&#39;s 没问题"}]}}"#.into())
    }))
    .await;
    let out = translate(&client, Kind::Google, &g, Some("gk"), "", "简体中文", &texts).await.unwrap();
    assert_eq!(out, ["你好", "It's 没问题"]);
    let sent = seen.lock().unwrap().clone();
    assert!(sent.starts_with("/language/translate/v2?key=gk"), "{sent}");
    assert!(sent.contains(r#""target":"zh-CN""#) && sent.contains(r#""q":["Hello","It's fine"]"#), "{sent}");

    // 微软：数组体，目标 zh-Hans，区域写进请求头
    let seen = Arc::new(Mutex::new(String::new()));
    let s = seen.clone();
    let m = mock(Arc::new(move |path, body, _| {
        *s.lock().unwrap() = format!("{path} {}", String::from_utf8_lossy(body));
        (200, r#"[{"translations":[{"text":"你好","to":"zh-Hans"}]},{"translations":[{"text":"没问题","to":"zh-Hans"}]}]"#.into())
    }))
    .await;
    let out = translate(&client, Kind::Microsoft, &m, Some("mk"), "eastasia", "简体中文", &texts).await.unwrap();
    assert_eq!(out, ["你好", "没问题"]);
    let sent = seen.lock().unwrap().clone();
    assert!(sent.contains("api-version=3.0") && sent.contains("to=zh-Hans") && sent.contains(r#"[{"Text":"Hello"},{"Text":"It's fine"}]"#), "{sent}");

    // DeepL：免费版密钥（:fx）在没填地址时走 api-free；这里填了本机地址，检查体和结果
    let seen = Arc::new(Mutex::new(String::new()));
    let s = seen.clone();
    let d = mock(Arc::new(move |path, body, _| {
        *s.lock().unwrap() = format!("{path} {}", String::from_utf8_lossy(body));
        (200, r#"{"translations":[{"text":"你好"},{"text":"没问题"}]}"#.into())
    }))
    .await;
    let out = translate(&client, Kind::DeepL, &d, Some("dk:fx"), "", "简体中文", &texts).await.unwrap();
    assert_eq!(out, ["你好", "没问题"]);
    let sent = seen.lock().unwrap().clone();
    assert!(sent.starts_with("/v2/translate") && sent.contains(r#""target_lang":"ZH-HANS""#), "{sent}");

    // DeepLX：一次一段，地址就是完整接口
    let calls = Arc::new(Mutex::new(0usize));
    let c = calls.clone();
    let x = mock(Arc::new(move |path, body, _| {
        assert_eq!(path, "/translate");
        *c.lock().unwrap() += 1;
        let v: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(v["target_lang"], "ZH");
        (200, format!(r#"{{"code":200,"data":"译:{}"}}"#, v["text"].as_str().unwrap()))
    }))
    .await;
    let out = translate(&client, Kind::DeepLX, &format!("{x}/translate"), None, "", "简体中文", &texts).await.unwrap();
    assert_eq!(out, ["译:Hello", "译:It's fine"]);
    assert_eq!(*calls.lock().unwrap(), 2);

    // 缺密钥 / 不支持的语言：本地就报错，不发请求
    assert!(translate(&client, Kind::Google, &g, None, "", "简体中文", &texts).await.unwrap_err().message.contains("密钥"));
    assert!(translate(&client, Kind::DeepL, &d, Some("k"), "", "泰语", &texts).await.is_err());
}

#[tokio::test]
async fn pool_routes_translation_to_machine_translation_but_skips_it_for_other_requests() {
    use crate::mt::Kind;
    let dl = mock(Arc::new(|_, _, _| (200, r#"{"translations":[{"text":"你好"},{"text":"世界"}]}"#.into()))).await;
    let chat_srv = mock(Arc::new(|_, _, _| (200, reply("SUMMARY")))).await;
    let pool = Arc::new(AiPool::default());
    let ep = pooled(vec![mt_cand("deepl", Kind::DeepL, &dl, Some("k"), ""), cand_for("llm", &chat_srv, "m")], pool.clone());

    // 翻译一批文字：机器翻译直接答，返回合法的 JSON 数组，map_cues_g 能照常解析
    let cues = vec![Cue { start: 0, end: 1, lines: vec!["Hello".into()] }, Cue { start: 1, end: 2, lines: vec!["World".into()] }];
    let t = translate_cues_g(&ep, "", "简体中文", &cues, 20, &[], &|_, _| Ok(())).await.unwrap();
    assert_eq!(t.texts, [Some("你好".to_string()), Some("世界".to_string())]);
    assert_eq!(pool.snapshot(crate::db::now())["deepl"].ok, 1);

    // 摘要这类不是“译一批文字”的请求：跳过机器翻译，交给后面的对话模型
    let out = chat(&ep, "", "Summarize the video.", "long transcript").await.unwrap();
    assert_eq!(out, "SUMMARY");
    assert!(!pool.snapshot(crate::db::now()).contains_key("deepl") || pool.snapshot(crate::db::now())["deepl"].fail == 0, "跳过不算失败");

    // 池里只有机器翻译时，摘要给出明确的提示而不是乱译
    let only = pooled(vec![mt_cand("deepl", Kind::DeepL, &dl, Some("k"), "")], Arc::new(AiPool::default()));
    let err = chat(&only, "", "Summarize the video.", "x").await.unwrap_err();
    assert!(err.message.contains("机器翻译"), "{}", err.message);

    // 机器翻译额度用完（456 / 429）→ 冷却并换对话模型继续译
    let dead = mock(Arc::new(|_, _, _| (456, r#"{"message":"Quota exceeded"}"#.into()))).await;
    let llm = mock(Arc::new(|_, _, _| (200, reply(r#"["你好","世界"]"#)))).await;
    let pool2 = Arc::new(AiPool::default());
    let ep2 = pooled(vec![mt_cand("deepl", Kind::DeepL, &dead, Some("k"), ""), cand_for("llm", &llm, "m")], pool2.clone());
    let t = translate_cues_g(&ep2, "", "简体中文", &cues, 20, &[], &|_, _| Ok(())).await.unwrap();
    assert_eq!(t.texts[0].as_deref(), Some("你好"));
    assert!(pool2.snapshot(crate::db::now())["deepl"].fail >= 1);
}

#[tokio::test]
async fn image_requests_go_to_vision_models_first() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let s1 = seen.clone();
    let text_only = mock(Arc::new(move |_, body, _| {
        let v: Value = serde_json::from_slice(body).unwrap();
        s1.lock().unwrap().push(v["model"].as_str().unwrap().to_string());
        (400, r#"{"error":{"message":"image input not supported"}}"#.into())
    }))
    .await;
    let s2 = seen.clone();
    let vision = mock(Arc::new(move |_, body, _| {
        let v: Value = serde_json::from_slice(body).unwrap();
        s2.lock().unwrap().push(v["model"].as_str().unwrap().to_string());
        (200, reply("看到了"))
    }))
    .await;
    // 纯文字模型优先级更高，但请求带图片：先试能看图的
    let ep = pooled(vec![cand_for("txt", &text_only, "qwen2.5-7b-instruct"), cand_for("vl", &vision, "qwen-vl-max")], Arc::new(AiPool::default()));
    let out = chat_img(&ep, "", "s", "看图", "data:image/png;base64,AAAA").await.unwrap();
    assert_eq!(out, "看到了");
    assert_eq!(*seen.lock().unwrap(), vec!["qwen-vl-max"], "没有先浪费一次在纯文字模型上");
    // 池里一个能看图的都没有：明确告诉用户
    let only_text = pooled(vec![cand_for("txt", &text_only, "qwen2.5-7b-instruct")], Arc::new(AiPool::default()));
    let err = chat_img(&only_text, "", "s", "看图", "data:image/png;base64,AAAA").await.unwrap_err();
    assert!(err.message.contains("能看图"), "{}", err.message);
}

#[test]
fn monthly_character_quota_takes_a_service_out_of_rotation() {
    use crate::ai_pool::{month_of, rank, Cand, Task};
    let now = 1_793_990_000; // 2026-11
    assert_eq!(month_of(now), 2026 * 12 + 10);
    let pool = AiPool::default();
    pool.record_chars("deepl", 499_990, now);
    pool.record_chars("deepl", 20, now);
    assert_eq!(pool.chars_used("deepl", now), 500_010);
    assert_eq!(pool.chars_used("deepl", now + 40 * 86400), 0, "下个月重新算");
    let c = |id: &str, limit: u64, used: u64| Cand {
        id: id.into(),
        priority: 10,
        daily_limit: 0,
        monthly_chars: limit,
        chars_used: used,
        tasks: vec![],
        usable: true,
    };
    let cands = vec![c("deepl", 500_000, pool.chars_used("deepl", now)), c("llm", 0, 0)];
    let order = rank(&cands, &Default::default(), Task::Translate, now);
    assert_eq!(order, vec![1], "本月额度用完的机器翻译不再被挑中");
}
