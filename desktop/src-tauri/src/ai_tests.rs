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
    Endpoint { base: base.to_string(), key: Some("sk-test".into()), client: reqwest::Client::builder().no_proxy().build().unwrap(), retry_ms: 5 }
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
    assert_eq!((s.batch_size, s.stt_engine.as_str(), s.target_lang.as_str()), (5, "api", "简体中文"));
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
