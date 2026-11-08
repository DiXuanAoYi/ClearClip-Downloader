//! 写作助手引擎：动作 + 四组修饰（长度 / 正式程度 / 格式 / 语气）合成提示词，
//! 改写结果的护栏检查，语法检查的结构化解析，以及“加入统计数据”的开放数据检索（世界银行开放数据）。
//! 这里只放纯逻辑，方便测试；网络和模型调用在 `vsum.rs`。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::Endpoint;
use crate::error::{AppError, AppResult};

/// 一次写作请求的选项（扩展 / 桌面端共用）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Opts {
    /// rewrite / translate / summarize / optimize / continue / shorten / stats / detail / humor / polite /
    /// plain / more_info / engaging / more_detail / persuasive / apologetic / sincere / concise
    pub action: String,
    /// auto / short / medium / long
    pub length: String,
    /// auto / casual / neutral / formal
    pub formality: String,
    /// auto / email / message / comment / paragraph / article / blog / idea / outline / twitter / reddit / facebook / linkedin，
    /// 或 `custom` 配合 `custom_format`
    pub format: String,
    pub custom_format: String,
    /// auto / warm / humorous / caring / humble / optimistic / empathetic / frank / sincere，或 `custom` 配合 `custom_tone`
    pub tone: String,
    pub custom_tone: String,
    /// 用户的额外要求
    pub extra: String,
    /// 翻译的目标语言（如 English / 简体中文）；留空 = 中英互译
    pub target: String,
    /// 生成几个候选（1 到 3）
    pub candidates: u8,
    /// local_first（默认）/ local_only / cloud_only
    pub route: String,
    /// stats：用户勾选的、来自开放数据库的数据
    pub facts: Vec<Fact>,
    /// 输入框所在页面的网址 / 标题，帮助判断场景
    pub page: String,
    /// 帮我回（action = reply）和微调（action = tweak）的选项
    pub reply: Reply,
}

/// “帮我回”：对方是谁 × 我的意思 × 怎么说（语气走 `Opts.tone`）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Reply {
    /// superior / colleague / client / friend / family / stranger；空 = 让模型自己判断
    pub relation: String,
    /// agree / conditional / later / ask / decline；空 = 写一个得体的回复
    pub intent: String,
    /// conditional：条件；later：什么时候答复；decline：理由（可空）
    pub detail: String,
    /// decline 时给的替代：another_time / someone_else；空 = 不给
    pub alt: String,
    /// ask：要问对方的问题
    pub questions: Vec<String>,
    /// 前面的聊天，只用来理解，不回复它们
    pub context: String,
    /// 这一版的说法：steady / warm / brief（后端按版本填，界面不用传）
    pub angle: String,
    /// 来源是邮件程序：写成带称呼和落款的邮件
    pub email: bool,
    /// tweak：softer / firmer / shorter / plain / emoji
    pub tweak: String,
    /// tweak：当初收到的消息（帮助保持意思）
    pub received: String,
}

pub const RELATIONS: [&str; 6] = ["superior", "colleague", "client", "friend", "family", "stranger"];
pub const INTENTS: [&str; 5] = ["agree", "conditional", "later", "ask", "decline"];
pub const TWEAKS: [&str; 5] = ["softer", "firmer", "shorter", "plain", "emoji"];
/// 帮我回一次给三版，各有一个说法
pub const ANGLES: [(&str, &str); 3] = [("steady", "稳妥"), ("warm", "热情"), ("brief", "简短")];

/// 一条来自开放数据库的数据。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Fact {
    pub text: String,
    pub value: f64,
    pub year: String,
    pub country: String,
    pub indicator: String,
    pub code: String,
    pub source: String,
    pub url: String,
}

pub const ACTIONS: [&str; 20] = [
    "reply",
    "tweak",
    "rewrite",
    "translate",
    "summarize",
    "optimize",
    "continue",
    "shorten",
    "stats",
    "detail",
    "humor",
    "polite",
    "plain",
    "more_info",
    "engaging",
    "more_detail",
    "persuasive",
    "apologetic",
    "sincere",
    "concise",
];

pub fn check_opts(kind: &str, text: &str, o: &Opts) -> AppResult<()> {
    if text.trim().is_empty() {
        return Err(AppError::invalid("没有可处理的文字。"));
    }
    if text.chars().count() > 20_000 {
        return Err(AppError::invalid("文字太长（写作助手一次最多 2 万字），请只选中一部分。"));
    }
    if kind == "write" {
        if !ACTIONS.contains(&o.action.as_str()) {
            return Err(AppError::invalid("不支持的写作动作"));
        }
        if o.action == "stats" && o.facts.is_empty() {
            return Err(AppError::invalid("请先检索并勾选要加入的数据。"));
        }
    }
    if kind == "write" && o.action == "reply" {
        let r = &o.reply;
        if !r.relation.is_empty() && !RELATIONS.contains(&r.relation.as_str()) {
            return Err(AppError::invalid("不支持的对方身份"));
        }
        if !r.intent.is_empty() && !INTENTS.contains(&r.intent.as_str()) {
            return Err(AppError::invalid("不支持的回复意思"));
        }
        if r.intent == "conditional" && r.detail.trim().is_empty() {
            return Err(AppError::invalid("请先写上条件（比如“下周一调休一天”）。"));
        }
        if r.intent == "ask" && r.questions.iter().all(|q| q.trim().is_empty()) {
            return Err(AppError::invalid("请至少勾选或写上一个要问的问题。"));
        }
        if r.questions.len() > 8 || r.questions.iter().any(|q| q.chars().count() > 100) {
            return Err(AppError::invalid("要问的问题太多或太长（最多 8 个，每个 100 字）。"));
        }
        if r.context.chars().count() > 6000 {
            return Err(AppError::invalid("前面的聊天太长（最多 6000 字），只贴最近几条就够了。"));
        }
    }
    if kind == "write" && o.action == "tweak" && !TWEAKS.contains(&o.reply.tweak.as_str()) {
        return Err(AppError::invalid("不支持的微调方式"));
    }
    for (s, max) in [(&o.custom_format, 200), (&o.custom_tone, 200), (&o.extra, 900), (&o.reply.detail, 200), (&o.reply.received, 4000)] {
        if s.chars().count() > max {
            return Err(AppError::invalid(format!("自定义要求太长（上限 {max} 字）。")));
        }
    }
    Ok(())
}

fn action_line(o: &Opts) -> String {
    match o.action.as_str() {
        "translate" => {
            if o.target.trim().is_empty() {
                "Translate the text: Chinese -> English, any other language -> Simplified Chinese. Keep the meaning, tone and layout.".into()
            } else {
                format!("Translate the text into {}. Keep the meaning, tone and layout.", o.target.trim())
            }
        }
        "summarize" => "Summarize the text: keep the key points and drop the rest.".into(),
        "optimize" => "Improve the text: fix mistakes, smooth the wording and make it clearer, without changing the meaning.".into(),
        "continue" => "Continue writing from where the text stops, in the same voice and direction. Output ONLY the new continuation, not the original text.".into(),
        "shorten" => "Make the text clearly shorter while keeping the essential meaning.".into(),
        "stats" => "Work the PROVIDED FACTS naturally into the text as supporting evidence. Use ONLY the numbers in PROVIDED FACTS, exactly as given (value, year, place). Add a short source note such as (Source: World Bank, 2023) after each fact you use. Do not add any other statistic.".into(),
        "detail" => "Add concrete details and examples that are implied by the text. Do not invent facts, names or numbers.".into(),
        "humor" => "Add light, tasteful humor without hurting clarity.".into(),
        "polite" => "Make the text more polite and courteous.".into(),
        "plain" => "Remove jargon: replace specialist terms with everyday words, or explain them briefly.".into(),
        "more_info" => "Make the text more informative by explaining the points that are only hinted at. Do not invent facts or numbers.".into(),
        "engaging" => "Make the text more engaging and vivid, so the reader wants to keep reading.".into(),
        "more_detail" => "Make the text more detailed and specific where it is vague. Do not invent facts or numbers.".into(),
        "persuasive" => "Make the text more persuasive with clearer reasoning and stronger wording, without exaggeration or false claims.".into(),
        "apologetic" => "Make the text sincerely apologetic and take responsibility without excess.".into(),
        "sincere" => "Make the text more sincere and genuine.".into(),
        "concise" => "Make the text concise: cut filler and repetition, keep every important point.".into(),
        "reply" => reply_lines(&o.reply),
        "tweak" => format!(
            "The text is a reply the user is about to send. Adjust it: {}. Keep its meaning, its answer (yes / no / not yet / questions), every time, number, name and condition. Output ONLY the adjusted reply.",
            tweak_line(&o.reply.tweak)
        ),
        _ => "Rewrite the text so it reads better while keeping the meaning.".into(),
    }
}

fn relation_line(v: &str) -> Option<&'static str> {
    match v {
        "superior" => Some("The other person is the user's boss or superior."),
        "colleague" => Some("The other person is the user's colleague."),
        "client" => Some("The other person is the user's client or customer."),
        "friend" => Some("The other person is the user's friend."),
        "family" => Some("The other person is a family member of the user."),
        "stranger" => Some("The user does not know the other person well."),
        _ => None,
    }
}

fn tweak_line(v: &str) -> &'static str {
    match v {
        "softer" => "make it softer and gentler",
        "firmer" => "make it firmer and more direct, still polite",
        "shorter" => "make it clearly shorter",
        "plain" => "remove pleasantries and filler, get straight to the point",
        "emoji" => "add one or two fitting emoji, nothing else changes",
        _ => "make it read better",
    }
}

/// 帮我回的提示词：收到的消息 + 我的意思 + 对方是谁 + 这一版的说法。
fn reply_lines(r: &Reply) -> String {
    let mut l = vec!["The text is a message the user RECEIVED. Write the user's reply to it. Reply in the same language as the received message. Output ONLY the reply, without quoting the received message.".to_string()];
    let detail = r.detail.trim();
    match r.intent.as_str() {
        "agree" => l.push("The user's answer: YES, they agree / accept.".into()),
        "conditional" => {
            l.push(format!("The user's answer: YES, but on this condition: {detail}. Agree clearly, then raise the condition politely as a request the other person can accept."))
        }
        "later" => {
            let when = if detail.is_empty() { "soon".to_string() } else { detail.to_string() };
            l.push(format!("The user's answer: NOT DECIDED YET. Neither agree nor refuse; say they need a little time and will answer {when}."))
        }
        "ask" => {
            let qs: Vec<String> = r.questions.iter().map(|q| q.trim()).filter(|q| !q.is_empty()).map(|q| format!("- {q}")).collect();
            l.push(format!(
                "The user's answer: they need to CLARIFY before answering. Ask exactly these points, nothing more, and do not agree or refuse yet:\n{}",
                qs.join("\n")
            ))
        }
        "decline" => {
            l.push("The user's answer: NO, they decline.".into());
            if detail.is_empty() {
                l.push("No reason was given: do NOT invent one — just say it does not work this time.".into());
            } else {
                l.push(format!("The reason to give (use only this): {detail}."));
            }
            match r.alt.as_str() {
                "another_time" => l.push("Offer to do it another time.".into()),
                "someone_else" => l.push("Suggest that someone else could help (no names unless the user gave one).".into()),
                _ => {}
            }
        }
        _ => l.push("The user's intent may be given in the extra request; if not, write a sensible, polite reply.".into()),
    }
    l.extend(relation_line(&r.relation).map(str::to_string));
    match r.angle.as_str() {
        "steady" => l.push("This version: steady and safe — the most appropriate wording.".into()),
        "warm" => l.push("This version: warmer and more enthusiastic.".into()),
        "brief" => l.push("This version: as short as possible while complete.".into()),
        _ => {}
    }
    if r.email {
        l.push("Format: an email reply with a suitable greeting and sign-off; if a name is unknown, use a neutral greeting instead of a placeholder.".into());
    }
    l.push("Use only the reasons, times, conditions and promises the user gave. Never invent excuses, facts, names, numbers or commitments.".into());
    l.join("\n")
}

/// 帮我回 / 微调的用户消息：前面的聊天（只用来理解）+ 要回的消息。
fn reply_message(o: &Opts, text: &str) -> String {
    let r = &o.reply;
    if o.action == "tweak" {
        let rec = r.received.trim();
        return if rec.is_empty() { text.trim().to_string() } else { format!("RECEIVED MESSAGE (for context):\n{rec}\n\nREPLY TO ADJUST:\n{}", text.trim()) };
    }
    let ctx = r.context.trim();
    if ctx.is_empty() {
        text.trim().to_string()
    } else {
        format!("EARLIER CONVERSATION (for understanding only, do not reply to it):\n{ctx}\n\nMESSAGE TO REPLY TO:\n{}", text.trim())
    }
}

/// 帮我回的结果里可以出现的东西：收到的消息、前面的聊天和用户自己写的。
pub fn reply_sources(o: &Opts, text: &str) -> String {
    let r = &o.reply;
    [text, &r.context, &r.detail, &r.received, &o.extra, &r.questions.join("\n")].join("\n")
}

// ---------- 帮我回：先看懂 ----------

pub fn reply_read_system() -> String {
    "You help the user understand a message they RECEIVED before replying. Reply with ONLY a JSON object:\n\
{\"ask\":\"what the other person wants from the user, at most 12 Chinese characters, e.g. 请你周六加班\",\
\"facts\":\"one short Chinese clause on the details given and the tone, e.g. 时间已给出（9:00–15:00），语气客气、有点急\",\
\"unclear\":[\"up to 4 points the user would need to ask about before answering, in Chinese, short, most important first; empty if everything is clear\"],\
\"relation\":\"superior|colleague|client|friend|family|stranger|\",\
\"who\":\"how the user should address the other person if it is clear from the conversation (e.g. 王经理), else empty\"}\n\
Base everything only on the text; do not guess facts that are not there."
        .into()
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ReplyRead {
    pub ask: String,
    pub facts: String,
    pub unclear: Vec<String>,
    pub relation: String,
    pub who: String,
}

pub fn parse_reply_read(resp: &str) -> ReplyRead {
    let raw = crate::ai::strip_fences(resp);
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').map(|i| i + 1).unwrap_or(raw.len());
    let v: Value = serde_json::from_str(raw.get(start..end).unwrap_or("")).unwrap_or(Value::Null);
    let g = |k: &str, n: usize| v.get(k).and_then(Value::as_str).unwrap_or("").trim().trim_end_matches(['。', '.']).chars().take(n).collect::<String>();
    let relation = g("relation", 20);
    ReplyRead {
        ask: g("ask", 30),
        facts: g("facts", 80),
        unclear: v
            .get("unclear")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(|q| q.trim().chars().take(60).collect::<String>()).filter(|q| !q.is_empty()).take(4).collect())
            .unwrap_or_default(),
        relation: if RELATIONS.contains(&relation.as_str()) { relation } else { String::new() },
        who: g("who", 20),
    }
}

fn length_line(v: &str) -> Option<&'static str> {
    match v {
        "short" => Some("Length: short."),
        "medium" => Some("Length: medium."),
        "long" => Some("Length: long and full."),
        _ => None,
    }
}

fn formality_line(v: &str) -> Option<&'static str> {
    match v {
        "casual" => Some("Formality: casual, conversational."),
        "neutral" => Some("Formality: neutral."),
        "formal" => Some("Formality: formal and professional."),
        _ => None,
    }
}

fn format_line(o: &Opts) -> Option<String> {
    let s = match o.format.as_str() {
        "email" => "Format: an email (greeting, body, closing).",
        "message" => "Format: a short chat message.",
        "comment" => "Format: a comment under a post.",
        "paragraph" => "Format: one well-formed paragraph.",
        "article" => "Format: an article with a clear structure.",
        "blog" => "Format: a blog post with a lively opening.",
        "idea" => "Format: a brief idea note.",
        "outline" => "Format: an outline with headings and bullet points.",
        "twitter" => "Format: a tweet (short, at most 280 characters, hashtags only if natural).",
        "reddit" => "Format: a Reddit post or comment in a natural community voice.",
        "facebook" => "Format: a Facebook post.",
        "linkedin" => "Format: a LinkedIn post, professional but human.",
        "custom" if !o.custom_format.trim().is_empty() => return Some(format!("Format: {}.", o.custom_format.trim())),
        _ => return None,
    };
    Some(s.to_string())
}

fn tone_line(o: &Opts) -> Option<String> {
    let s = match o.tone.as_str() {
        "warm" => "Tone: warm and enthusiastic.",
        "humorous" => "Tone: humorous.",
        "caring" => "Tone: caring.",
        "humble" => "Tone: humble.",
        "optimistic" => "Tone: optimistic.",
        "empathetic" => "Tone: empathetic.",
        "frank" => "Tone: frank and direct.",
        "sincere" => "Tone: sincere.",
        // 写作窗“怎么说”这一排（帮我回和改写润色共用）
        "proper" => "Tone: formal and proper — respectful, well-mannered, clear.",
        "eq" => "Tone: emotionally intelligent — acknowledge the other person's needs and feelings, keep the relationship good, tactful and considerate, never servile.",
        "banter" => "Tone: playful banter — pick up on what they said with light, friendly humor, while the actual answer stays clear.",
        "brief" => "Tone: brief and decisive — no pleasantries beyond the minimum.",
        "heartfelt" => "Tone: warm and sincere, like a real person who cares.",
        "custom" if !o.custom_tone.trim().is_empty() => return Some(format!("Tone: {}.", o.custom_tone.trim())),
        _ => return None,
    };
    Some(s.to_string())
}

/// 把动作和四组修饰合成一条系统提示词。
pub fn system_prompt(o: &Opts) -> String {
    let mut lines = vec![
        "You are a careful writing assistant inside a text box. Return ONLY the resulting text: no explanations, no quotation marks around it, no markdown fences, no preface.".to_string(),
        action_line(o),
    ];
    if o.action != "translate" {
        lines.push("Write in the same language as the input text.".into());
    }
    lines.extend(length_line(&o.length).map(str::to_string));
    lines.extend(formality_line(&o.formality).map(str::to_string));
    lines.extend(format_line(o));
    lines.extend(tone_line(o));
    if !o.extra.trim().is_empty() {
        lines.push(format!("Extra request from the user: {}", o.extra.trim()));
    }
    if !matches!(o.action.as_str(), "reply" | "tweak") {
        lines.push("Keep unchanged: numbers, dates, URLs, @mentions, #hashtags, email addresses, code and inline code, product and person names. Never invent facts, quotes or statistics. Keep line breaks where the input has them unless the format needs a new structure.".into());
    }
    let auto_all = [&o.length, &o.formality, &o.format, &o.tone].iter().all(|v| v.is_empty() || v.as_str() == "auto");
    if auto_all {
        lines.push("Where length, formality, format or tone are not specified, infer them from the text and where it is being written.".into());
    }
    if !o.page.trim().is_empty() {
        lines.push(format!("Where the text is being written: {}", o.page.trim().chars().take(200).collect::<String>()));
    }
    if o.action == "stats" {
        // 数据放在用户消息里，这里只说明规则
        lines.push("The user message has two parts: PROVIDED FACTS and TEXT.".into());
    }
    lines.join("\n")
}

/// 用户消息：原文，stats 动作时先放数据。
pub fn user_message(o: &Opts, text: &str) -> String {
    if matches!(o.action.as_str(), "reply" | "tweak") {
        return reply_message(o, text);
    }
    if o.action == "stats" {
        let facts: Vec<String> =
            o.facts.iter().map(|f| format!("- {} (value={}, year={}, place={}, source={})", f.text, f.value, f.year, f.country, f.source)).collect();
        return format!("PROVIDED FACTS:\n{}\n\nTEXT:\n{}", facts.join("\n"), text.trim());
    }
    text.trim().to_string()
}

// ---------- 护栏 ----------

/// 原文里必须原样保留的东西：网址、@名字、#话题、邮箱、数字（含小数、千分位、百分号）。
pub fn protected_tokens(text: &str) -> Vec<String> {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"https?://[^\s，。）)\]]+|[\w.+-]+@[\w-]+(?:\.[\w-]+)+|@[\w\u{4e00}-\u{9fff}]+|#[\w\u{4e00}-\u{9fff}]+|\d[\d,]*(?:\.\d+)?%?")
            .unwrap()
    });
    let mut out: Vec<String> = Vec::new();
    for m in RE.find_iter(text) {
        let t = m.as_str().trim_end_matches(['.', ',']).to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Guard {
    /// 原文里有、结果里不见了的内容
    pub missing: Vec<String>,
    /// 结果长度 / 原文长度
    pub ratio: f64,
    /// 结果里有、原文和用户给的内容里都没有的数字 / 时间 / 网址（帮我回用：可能是模型编的）
    pub added: Vec<String>,
    /// 需要提醒用户：缺东西，或长度差得离谱
    pub warn: bool,
}

/// 动作本来就会删减内容时，“缺东西”不算问题。
fn may_drop(action: &str) -> bool {
    matches!(action, "summarize" | "shorten" | "concise" | "continue" | "translate" | "reply" | "tweak")
}

pub fn guard(action: &str, src: &str, out: &str) -> Guard {
    let (a, b) = (src.trim().chars().count().max(1) as f64, out.trim().chars().count() as f64);
    let ratio = (b / a * 100.0).round() / 100.0;
    let missing: Vec<String> = if may_drop(action) { vec![] } else { protected_tokens(src).into_iter().filter(|t| !out.contains(t.as_str())).collect() };
    let off = match action {
        "summarize" | "shorten" | "concise" | "continue" | "translate" => ratio > 4.0,
        // 回复的长短和收到的消息没有关系
        "reply" | "tweak" => false,
        _ => !(0.25..=4.0).contains(&ratio),
    };
    Guard { warn: !missing.is_empty() || off, missing, ratio, added: vec![] }
}

/// 帮我回 / 微调：不看长短、不要求带回原文，只查有没有凭空多出来的数字、时间、网址。
pub fn guard_reply(sources: &str, out: &str) -> Guard {
    let added: Vec<String> = protected_tokens(out).into_iter().filter(|t| !sources.contains(t.as_str())).collect();
    let ratio = ((out.trim().chars().count() as f64 / sources.trim().chars().count().max(1) as f64) * 100.0).round() / 100.0;
    Guard { warn: !added.is_empty(), missing: vec![], ratio, added }
}

/// 去掉模型爱加的包装：代码围栏、开头的“改写后：”、整段外层引号。
pub fn clean_output(s: &str) -> String {
    let mut t = crate::ai::strip_fences(s).trim().to_string();
    for p in ["改写后：", "改写后:", "结果：", "结果:", "Rewritten:", "Result:"] {
        if let Some(r) = t.strip_prefix(p) {
            t = r.trim().to_string();
        }
    }
    let n = t.chars().count();
    if n >= 2 {
        let (f, l) = (t.chars().next().unwrap_or(' '), t.chars().last().unwrap_or(' '));
        let pair = matches!((f, l), ('"', '"') | ('“', '”') | ('「', '」'));
        // 原文自己带引号的情况很少见；整段被引号包住基本是模型的包装
        if pair && !t[f.len_utf8()..t.len() - l.len_utf8()].contains(f) {
            t = t.chars().skip(1).take(n - 2).collect();
        }
    }
    t.trim().to_string()
}

// ---------- 个人风格 ----------

/// 从用户自己写的几段文字里总结写作风格，结果可直接当作“我的写作风格”使用。
pub fn style_system() -> String {
    "Read the user's own writing samples and describe the author's writing style as a short instruction another writer could follow: \
typical sentence length, tone, formality, vocabulary habits, punctuation and emoji habits, how they open and close. \
Write the description in Chinese, at most 120 characters, in imperative form (for example: 口语化，短句为主，少用感叹号，结尾常带一句关心). \
Reply with ONLY the description, no quotes or labels."
        .into()
}

/// 整理模型给的风格描述：去掉包装、截到 300 字。
pub fn clean_style(s: &str) -> String {
    clean_output(s).trim().chars().take(300).collect()
}

// ---------- 语法检查 ----------

pub fn grammar_system() -> String {
    "You are a proofreader for Chinese and English text. Find real errors only: wrong characters, typos, grammar mistakes, missing or doubled words, wrong punctuation, awkward or unnatural phrasing. \
Do not rewrite for style unless the phrasing is clearly wrong; do not touch proper nouns, code, URLs or numbers.\n\
Reply with ONLY a JSON object: {\"issues\":[{\"from\":\"the exact wrong snippet copied from the text\",\"to\":\"the corrected snippet\",\"reason\":\"one short sentence in the language of the text\",\"type\":\"error|awkward|style\"}]}.\n\
Keep each snippet as short as possible but unique enough to find in the text. If there is nothing to fix, reply {\"issues\":[]}.".into()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Issue {
    pub from: String,
    pub to: String,
    pub reason: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// 在原文里的字符位置 [start, end)
    pub start: usize,
    pub end: usize,
}

fn kind_of(s: &str) -> &'static str {
    match s {
        "awkward" => "awkward",
        "style" => "style",
        _ => "error",
    }
}

/// 解析模型返回的语法问题，丢掉在原文里找不到的、没有改动的，并算出位置。
pub fn parse_issues(resp: &str, text: &str) -> Vec<Issue> {
    let raw = crate::ai::strip_fences(resp);
    let v: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let arr = v.get("issues").and_then(Value::as_array).or_else(|| v.as_array()).cloned().unwrap_or_default();
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Issue> = Vec::new();
    for it in arr {
        let g = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (from, to) = (g("from"), g("to"));
        if from.is_empty() || from == to {
            continue;
        }
        let from_chars: Vec<char> = from.chars().collect();
        // 同一个片段出现多次时，依次对应后面还没被占用的位置
        let mut pos = None;
        let mut i = 0;
        while i + from_chars.len() <= chars.len() {
            if chars[i..i + from_chars.len()] == from_chars[..] && !out.iter().any(|o| i < o.end && i + from_chars.len() > o.start) {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        let Some(start) = pos else { continue };
        out.push(Issue {
            from,
            to,
            reason: g("reason").chars().take(160).collect(),
            kind: kind_of(&g("type")).to_string(),
            start,
            end: start + from_chars.len(),
        });
    }
    out.sort_by_key(|i| i.start);
    out
}

/// 不用模型也能马上发现的问题：连续重复的虚词、重复标点、英文单词重复。
pub fn quick_issues(text: &str) -> Vec<Issue> {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"([的了是在和就也都]){2}|([，。！？；：]){2}|\b([A-Za-z]{2,})\s+\b([A-Za-z]{2,})\b").unwrap());
    let mut out = Vec::new();
    let mut byte_to_char = vec![0usize; text.len() + 1];
    for (ci, (bi, c)) in text.char_indices().enumerate() {
        for k in 0..c.len_utf8() {
            byte_to_char[bi + k] = ci;
        }
    }
    byte_to_char[text.len()] = text.chars().count();
    for caps in RE.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let s = m.as_str();
        let (fix, reason): (String, &str) = if let (Some(a), Some(b)) = (caps.get(3), caps.get(4)) {
            if !a.as_str().eq_ignore_ascii_case(b.as_str()) {
                continue;
            }
            (a.as_str().to_string(), "单词重复")
        } else if caps.get(1).is_some() {
            let mut it = s.chars();
            let first = it.next().unwrap_or(' ');
            if it.next() != Some(first) {
                continue;
            }
            (first.to_string(), "字重复")
        } else {
            let first = s.chars().next().unwrap_or(' ');
            if s.chars().nth(1) != Some(first) {
                continue;
            }
            (first.to_string(), "标点重复")
        };
        out.push(Issue {
            from: s.to_string(),
            to: fix,
            reason: reason.into(),
            kind: "error".into(),
            start: byte_to_char[m.start()],
            end: byte_to_char[m.end()],
        });
    }
    out
}

/// 合并规则发现的和模型发现的问题，位置重叠时留模型的。
pub fn merge_issues(quick: Vec<Issue>, model: Vec<Issue>) -> Vec<Issue> {
    let mut all = model;
    for q in quick {
        if !all.iter().any(|m| q.start < m.end && q.end > m.start) {
            all.push(q);
        }
    }
    all.sort_by_key(|i| i.start);
    all
}

/// 把所有建议一次应用到原文上（从后往前替换，位置不会错乱）。
pub fn apply_issues(text: &str, issues: &[Issue]) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by_key(|i| std::cmp::Reverse(i.start));
    for i in sorted {
        if i.end <= chars.len() && i.start <= i.end {
            chars.splice(i.start..i.end, i.to.chars());
        }
    }
    chars.into_iter().collect()
}

// ---------- 模型路由：本地优先 ----------

/// 按设置整理候选服务：本地优先（默认）/ 仅本地 / 仅云端。
pub fn route(mut ep: Endpoint, mode: &str) -> AppResult<Endpoint> {
    let is_local = |b: &str| crate::ai::is_local_url(b);
    if ep.cands.is_empty() {
        let local = is_local(&ep.base);
        return match mode {
            "local_only" if !local => {
                Err(AppError::invalid("已设为“仅本地模型”，但当前没有配置本地模型（如 Ollama）。请到设置里改为“本地优先”或添加本地模型。"))
            }
            "cloud_only" if local => Err(AppError::invalid("已设为“仅云端模型”，但当前只配置了本地模型。")),
            _ => Ok(ep),
        };
    }
    match mode {
        "local_only" => ep.cands.retain(|c| is_local(&c.base)),
        "cloud_only" => ep.cands.retain(|c| !is_local(&c.base)),
        _ => ep.cands.sort_by_key(|c| !is_local(&c.base)), // 稳定排序：本地的排前面，其余顺序不变
    }
    let Some(first) = ep.cands.first() else {
        return Err(AppError::invalid(if mode == "local_only" {
            "已设为“仅本地模型”，但模型池里没有本地服务。"
        } else {
            "已设为“仅云端模型”，但模型池里没有云端服务。"
        }));
    };
    ep.base = first.base.clone();
    ep.key = first.key.clone();
    ep.client = first.client.clone();
    Ok(ep)
}

// ---------- 统计数据：世界银行开放数据 ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    Count,
    Usd,
    Percent,
    Years,
    Per100,
    Tonnes,
    Per1000,
}

/// 检索时可选的指标（世界银行开放数据的指标代码）。
pub const CATALOG: [(&str, &str, Unit); 28] = [
    ("SP.POP.TOTL", "总人口", Unit::Count),
    ("SP.POP.GROW", "人口年增长率", Unit::Percent),
    ("SP.POP.65UP.TO.ZS", "65 岁及以上人口占比", Unit::Percent),
    ("SP.URB.TOTL.IN.ZS", "城镇人口占总人口比例", Unit::Percent),
    ("NY.GDP.MKTP.CD", "国内生产总值（现价美元）", Unit::Usd),
    ("NY.GDP.PCAP.CD", "人均国内生产总值（现价美元）", Unit::Usd),
    ("FP.CPI.TOTL.ZG", "通货膨胀率（消费者价格，年度）", Unit::Percent),
    ("NE.EXP.GNFS.ZS", "货物和服务出口占 GDP 比重", Unit::Percent),
    ("SL.UEM.TOTL.ZS", "失业率（占劳动力总数）", Unit::Percent),
    ("SL.TLF.CACT.ZS", "劳动参与率（15 岁以上）", Unit::Percent),
    ("SL.TLF.TOTL.IN", "劳动力总数", Unit::Count),
    ("SL.EMP.TOTL.SP.ZS", "就业人口占 15 岁以上人口比例", Unit::Percent),
    ("SI.POV.DDAY", "每天生活费低于国际贫困线的人口比例", Unit::Percent),
    ("SP.DYN.LE00.IN", "出生时预期寿命", Unit::Years),
    ("SP.DYN.IMRT.IN", "婴儿死亡率（每千名活产）", Unit::Per1000),
    ("SH.DYN.MORT", "五岁以下儿童死亡率（每千名活产）", Unit::Per1000),
    ("SH.XPD.CHEX.GD.ZS", "卫生支出占 GDP 比重", Unit::Percent),
    ("SE.ADT.LITR.ZS", "成人识字率", Unit::Percent),
    ("SE.XPD.TOTL.GD.ZS", "教育支出占 GDP 比重", Unit::Percent),
    ("GB.XPD.RSDV.GD.ZS", "研发支出占 GDP 比重", Unit::Percent),
    ("IT.NET.USER.ZS", "互联网使用者占人口比例", Unit::Percent),
    ("IT.CEL.SETS.P2", "每百人移动电话订阅数", Unit::Per100),
    ("IT.NET.BBND.P2", "每百人固定宽带订阅数", Unit::Per100),
    ("EG.ELC.ACCS.ZS", "通电人口占总人口比例", Unit::Percent),
    ("EN.ATM.CO2E.PC", "人均二氧化碳排放量（吨）", Unit::Tonnes),
    ("AG.LND.FRST.ZS", "森林面积占国土面积比例", Unit::Percent),
    ("SP.DYN.TFRT.IN", "总和生育率", Unit::Per100),
    ("EG.USE.ELEC.KH.PC", "人均用电量（千瓦时）", Unit::Count),
];

pub fn catalog_entry(code: &str) -> Option<(&'static str, &'static str, Unit)> {
    CATALOG.iter().copied().find(|(c, _, _)| *c == code)
}

pub fn stats_plan_system() -> String {
    let list: Vec<String> = CATALOG.iter().map(|(c, n, _)| format!("{c} = {n}")).collect();
    format!(
        "You help find open statistics to support a text. Read the text and choose at most 4 indicators from this list that would genuinely support it, and the countries they are about \
(ISO 3166 alpha-3 codes such as CHN, USA, DEU; use WLD for the world). If the text names no country, choose WLD, CHN and USA at most. If nothing in the list is relevant, return an empty list.\n\
INDICATORS:\n{}\n\
Reply with ONLY JSON: {{\"queries\":[{{\"indicator\":\"CODE\",\"countries\":[\"ISO3\"],\"why\":\"short reason in the language of the text\"}}]}}. Never invent codes.",
        list.join("\n")
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub code: String,
    pub countries: Vec<String>,
    pub why: String,
}

/// 解析检索计划；只留目录里有的指标和格式正确的国家代码。
pub fn parse_plan(resp: &str) -> Vec<Query> {
    let v: Value = serde_json::from_str(crate::ai::strip_fences(resp)).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for q in v.get("queries").and_then(Value::as_array).cloned().unwrap_or_default().iter().take(4) {
        let code = q.get("indicator").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if catalog_entry(&code).is_none() {
            continue;
        }
        let countries: Vec<String> = q
            .get("countries")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase()))
                    .take(3)
                    .collect()
            })
            .unwrap_or_default();
        if countries.is_empty() {
            continue;
        }
        out.push(Query { code, countries, why: q.get("why").and_then(Value::as_str).unwrap_or("").chars().take(120).collect() });
    }
    out
}

pub fn wb_url(q: &Query) -> String {
    format!("https://api.worldbank.org/v2/country/{}/indicator/{}?format=json&mrnev=1&per_page=20", q.countries.join(";"), q.code)
}

/// 数值的中文显示：人口 14.1 亿、金额 17.7 万亿美元、百分比 4.9%。
pub fn fmt_value(v: f64, unit: Unit) -> String {
    let big = |x: f64| -> String {
        let a = x.abs();
        if a >= 1e12 {
            format!("{:.2} 万亿", x / 1e12)
        } else if a >= 1e8 {
            format!("{:.2} 亿", x / 1e8)
        } else if a >= 1e4 {
            format!("{:.1} 万", x / 1e4)
        } else {
            format!("{x:.0}")
        }
    };
    match unit {
        Unit::Count => big(v),
        Unit::Usd => {
            if v.abs() >= 1e4 && v.abs() < 1e8 {
                format!("{v:.0} 美元")
            } else {
                format!("{} 美元", big(v))
            }
        }
        Unit::Percent => format!("{v:.1}%"),
        Unit::Years => format!("{v:.1} 岁"),
        Unit::Per100 => format!("{v:.1}"),
        Unit::Per1000 => format!("{v:.1}‰"),
        Unit::Tonnes => format!("{v:.2} 吨"),
    }
}

/// 解析世界银行接口的返回：`[ {分页}, [ {countryiso3code, country:{value}, date, value} … ] ]`。
pub fn parse_wb(body: &str, q: &Query) -> Vec<Fact> {
    let Some((name, ind_name, unit)) = catalog_entry(&q.code) else { return vec![] };
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let rows = v.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out: Vec<Fact> = Vec::new();
    for r in rows {
        let Some(value) = r.get("value").and_then(Value::as_f64) else { continue };
        let year = r.get("date").and_then(Value::as_str).unwrap_or("").to_string();
        let country = r.pointer("/country/value").and_then(Value::as_str).unwrap_or("").to_string();
        if year.is_empty() || country.is_empty() || out.iter().any(|f| f.country == country) {
            continue;
        }
        out.push(Fact {
            text: format!("{country} {year} 年{ind_name}：{}", fmt_value(value, unit)),
            value,
            year,
            country,
            indicator: ind_name.to_string(),
            code: name.to_string(),
            source: "世界银行开放数据".into(),
            url: format!("https://data.worldbank.org/indicator/{}", q.code),
        });
    }
    out
}

pub fn facts_json(facts: &[Fact]) -> Value {
    json!(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(action: &str) -> Opts {
        Opts { action: action.into(), ..Default::default() }
    }

    #[test]
    fn prompt_combines_action_and_modifiers() {
        let mut o = opts("concise");
        o.length = "short".into();
        o.formality = "formal".into();
        o.format = "email".into();
        o.tone = "custom".into();
        o.custom_tone = "像老朋友".into();
        let p = system_prompt(&o);
        assert!(p.contains("concise"));
        assert!(p.contains("Length: short"));
        assert!(p.contains("formal and professional"));
        assert!(p.contains("an email"));
        assert!(p.contains("像老朋友"));
        assert!(!p.contains("infer them from the text"));
    }

    #[test]
    fn all_auto_asks_model_to_infer() {
        let mut o = opts("rewrite");
        o.length = "auto".into();
        assert!(system_prompt(&o).contains("infer them from the text"));
    }

    #[test]
    fn translate_has_no_same_language_rule() {
        let mut o = opts("translate");
        o.target = "English".into();
        let p = system_prompt(&o);
        assert!(p.contains("into English"));
        assert!(!p.contains("same language as the input"));
    }

    #[test]
    fn tokens_are_found() {
        let t = protected_tokens("见 https://a.com/x，联系 @小明 或 me@x.org，增长 12.5%，共 1,200 人 #双十一");
        for want in ["https://a.com/x", "@小明", "me@x.org", "12.5%", "1,200", "#双十一"] {
            assert!(t.contains(&want.to_string()), "{want} 不在 {t:?}");
        }
    }

    #[test]
    fn guard_flags_lost_numbers() {
        let g = guard("rewrite", "销量增长 35%，共 120 台", "销量大幅增长，共 120 台");
        assert_eq!(g.missing, vec!["35%".to_string()]);
        assert!(g.warn);
        assert!(!guard("rewrite", "销量增长 35%", "销量增长 35%，势头不错").warn);
    }

    #[test]
    fn guard_allows_dropping_when_shortening() {
        let g = guard("summarize", "销量增长 35%，共 120 台，很好", "销量上涨");
        assert!(g.missing.is_empty());
        assert!(!g.warn);
    }

    #[test]
    fn guard_flags_extreme_length() {
        assert!(guard("rewrite", &"字".repeat(100), "字").warn);
    }

    #[test]
    fn clean_output_strips_wrappers() {
        assert_eq!(clean_output("```\n你好\n```"), "你好");
        assert_eq!(clean_output("改写后：你好"), "你好");
        assert_eq!(clean_output("“你好”"), "你好");
        assert_eq!(clean_output("“你好”和“再见”"), "“你好”和“再见”");
    }

    #[test]
    fn issues_locate_and_skip_unfound() {
        let text = "我们将于下周发步新产品";
        let resp = r#"{"issues":[{"from":"发步","to":"发布","reason":"错别字","type":"error"},{"from":"不存在","to":"x"},{"from":"同","to":"同"}]}"#;
        let v = parse_issues(resp, text);
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].start, v[0].end), (6, 8));
        assert_eq!(apply_issues(text, &v), "我们将于下周发布新产品");
    }

    #[test]
    fn repeated_snippets_map_to_distinct_positions() {
        let text = "的的一二的的";
        let resp = r#"[{"from":"的的","to":"的"},{"from":"的的","to":"的"}]"#;
        let v = parse_issues(resp, text);
        assert_eq!(v.iter().map(|i| i.start).collect::<Vec<_>>(), vec![0, 4]);
        assert_eq!(apply_issues(text, &v), "的一二的");
    }

    #[test]
    fn quick_rules_find_doubles() {
        let q = quick_issues("我们的的产品，，很好。 the the cat");
        let froms: Vec<&str> = q.iter().map(|i| i.from.as_str()).collect();
        assert!(froms.contains(&"的的"));
        assert!(froms.contains(&"，，"));
        assert!(froms.iter().any(|f| f.to_lowercase() == "the the"));
        assert_eq!(apply_issues("我们的的产品，，很好", &quick_issues("我们的的产品，，很好")), "我们的产品，很好");
        // 位置按字符算，不是字节
        assert_eq!(q[0].start, 2);
    }

    #[test]
    fn quick_rules_ignore_normal_text() {
        assert!(quick_issues("今天天气不错。我们去公园。Hello world.").is_empty());
    }

    #[test]
    fn merge_prefers_model_on_overlap() {
        let m = vec![Issue { from: "的的".into(), to: "的".into(), reason: "m".into(), kind: "error".into(), start: 2, end: 4 }];
        let q = quick_issues("我们的的产品，，很好");
        let merged = merge_issues(q, m);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].reason, "m");
    }

    #[test]
    fn plan_only_keeps_catalog_codes() {
        let resp = r#"{"queries":[{"indicator":"SP.POP.TOTL","countries":["chn","USA","xy"],"why":"人口"},{"indicator":"FAKE.CODE","countries":["CHN"]},{"indicator":"SL.UEM.TOTL.ZS","countries":[]}]}"#;
        let p = parse_plan(resp);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].countries, vec!["CHN", "USA"]);
        assert_eq!(wb_url(&p[0]), "https://api.worldbank.org/v2/country/CHN;USA/indicator/SP.POP.TOTL?format=json&mrnev=1&per_page=20");
    }

    #[test]
    fn world_bank_response_is_parsed() {
        let body = r#"[{"page":1,"pages":1,"per_page":20,"total":2},[
            {"indicator":{"id":"SL.UEM.TOTL.ZS","value":"x"},"country":{"id":"CN","value":"China"},"countryiso3code":"CHN","date":"2023","value":4.67},
            {"indicator":{"id":"SL.UEM.TOTL.ZS","value":"x"},"country":{"id":"US","value":"United States"},"countryiso3code":"USA","date":"2023","value":null}]]"#;
        let q = Query { code: "SL.UEM.TOTL.ZS".into(), countries: vec!["CHN".into(), "USA".into()], why: String::new() };
        let f = parse_wb(body, &q);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].country, "China");
        assert_eq!(f[0].year, "2023");
        assert!(f[0].text.contains("4.7%"));
        assert_eq!(f[0].url, "https://data.worldbank.org/indicator/SL.UEM.TOTL.ZS");
        assert!(parse_wb("not json", &q).is_empty());
    }

    #[test]
    fn values_are_formatted_in_chinese_units() {
        assert_eq!(fmt_value(1.4e9, Unit::Count), "14.00 亿");
        assert_eq!(fmt_value(1.77e13, Unit::Usd), "17.70 万亿 美元");
        assert_eq!(fmt_value(4.67, Unit::Percent), "4.7%");
        assert_eq!(fmt_value(78.2, Unit::Years), "78.2 岁");
    }

    #[test]
    fn stats_requires_facts_and_known_action() {
        assert!(check_opts("write", "文字", &opts("stats")).is_err());
        let mut o = opts("stats");
        o.facts = vec![Fact::default()];
        assert!(check_opts("write", "文字", &o).is_ok());
        assert!(check_opts("write", "文字", &opts("bogus")).is_err());
        assert!(check_opts("write", "  ", &opts("rewrite")).is_err());
    }

    #[test]
    fn stats_user_message_lists_facts() {
        let mut o = opts("stats");
        o.facts = vec![Fact {
            text: "China 2023 年失业率：4.7%".into(),
            value: 4.67,
            year: "2023".into(),
            country: "China".into(),
            source: "世界银行开放数据".into(),
            ..Default::default()
        }];
        let m = user_message(&o, "原文");
        assert!(m.contains("PROVIDED FACTS") && m.contains("4.67") && m.ends_with("原文"));
    }

    #[test]
    fn reply_answers_the_received_message() {
        let mut o = opts("reply");
        o.extra = "我想表达：周五可以，但要改到下午".into();
        let p = system_prompt(&o);
        assert!(p.contains("RECEIVED") && p.contains("reply") && p.contains("周五可以"), "用户的意图要进提示词");
        assert!(user_message(&o, "王经理：周五的评审能参加吗？").contains("王经理"));
        assert!(check_opts("write", "收到的消息", &o).is_ok());
        // 回复不要求带回原消息里的数字、网址，也允许比原消息长得多
        let g = guard("reply", "会议 3 号楼 502 室，见 https://a.b/c", "好的，我准时到。");
        assert!(g.missing.is_empty() && !g.warn);
        assert!(!guard("reply", "好", &"字".repeat(20)).warn);
    }

    #[test]
    fn help_me_reply_builds_intent_relation_and_tone() {
        let mut o = opts("reply");
        o.tone = "eq".into();
        o.reply =
            Reply {
                relation: "superior".into(), intent: "conditional".into(), detail: "下周一调休一天".into(), angle: "warm".into(), ..Default::default()
            };
        let p = system_prompt(&o);
        assert!(p.contains("on this condition: 下周一调休一天"), "条件要进提示词：{p}");
        assert!(p.contains("boss") && p.contains("emotionally intelligent") && p.contains("warmer"));
        assert!(p.contains("Never invent excuses"), "不替用户编理由");
        assert!(!p.contains("Keep line breaks"), "改写用的“原样保留”规则不放进回复");
        assert!(check_opts("write", "周六能来加班吗", &o).is_ok());
        // 有条件但没写条件、问清楚但没有问题：先拦下
        o.reply.detail.clear();
        assert!(check_opts("write", "周六能来加班吗", &o).is_err());
        o.reply = Reply { intent: "ask".into(), ..Default::default() };
        assert!(check_opts("write", "方案明天给我", &o).is_err());
        o.reply.questions = vec!["指哪一份方案".into(), " ".into()];
        assert!(check_opts("write", "方案明天给我", &o).is_ok());
        assert!(system_prompt(&o).contains("- 指哪一份方案"));
        o.reply.relation = "boss".into();
        assert!(check_opts("write", "方案明天给我", &o).is_err(), "不认识的身份");
    }

    #[test]
    fn declining_without_a_reason_does_not_invent_one() {
        let mut o = opts("reply");
        o.reply = Reply { intent: "decline".into(), alt: "another_time".into(), ..Default::default() };
        let p = system_prompt(&o);
        assert!(p.contains("NO, they decline") && p.contains("do NOT invent one") && p.contains("another time"));
        o.reply.detail = "那天要回老家".into();
        assert!(system_prompt(&o).contains("use only this): 那天要回老家"));
        o.reply = Reply { intent: "later".into(), detail: "明天".into(), email: true, ..Default::default() };
        let p = system_prompt(&o);
        assert!(p.contains("NOT DECIDED YET") && p.contains("answer 明天") && p.contains("email reply"));
    }

    #[test]
    fn earlier_chat_is_context_only() {
        let mut o = opts("reply");
        o.reply.context = "王经理：上周辛苦了".into();
        let u = user_message(&o, "周六能来吗？");
        assert!(u.starts_with("EARLIER CONVERSATION") && u.ends_with("MESSAGE TO REPLY TO:\n周六能来吗？"));
        let mut t = opts("tweak");
        t.reply = Reply { tweak: "softer".into(), received: "周六能来吗？".into(), ..Default::default() };
        assert!(system_prompt(&t).contains("softer and gentler"));
        assert!(user_message(&t, "可以。").contains("REPLY TO ADJUST:\n可以。"));
        assert!(check_opts("write", "可以。", &t).is_ok());
        t.reply.tweak = "louder".into();
        assert!(check_opts("write", "可以。", &t).is_err());
    }

    #[test]
    fn reply_guard_flags_numbers_nobody_gave() {
        let mut o = opts("reply");
        o.extra = "9 点准时到".into();
        o.reply.detail = "下周一调休".into();
        let src = reply_sources(&o, "周六 9 点到下午 3 点，方便加班吗？");
        let ok = guard_reply(&src, "王经理好，周六我 9 点到，下周一想调休一天。");
        assert!(!ok.warn && ok.added.is_empty());
        let bad = guard_reply(&src, "好的，周六 8 点到，加班费 200 元。");
        assert_eq!(bad.added, vec!["8".to_string(), "200".to_string()]);
        assert!(bad.warn);
    }

    #[test]
    fn reply_read_is_parsed_leniently() {
        let r = parse_reply_read("好的：\n```json\n{\"ask\":\"请你周六加班。\",\"facts\":\"时间已给出（9:00–15:00），语气客气\",\"unclear\":[\"几点结束\",\" \"],\"relation\":\"superior\",\"who\":\"王经理\"}\n```");
        assert_eq!(r.ask, "请你周六加班");
        assert_eq!(r.unclear, vec!["几点结束".to_string()]);
        assert_eq!((r.relation.as_str(), r.who.as_str()), ("superior", "王经理"));
        assert_eq!(parse_reply_read("{\"relation\":\"boss\"}").relation, "", "不认识的身份丢掉");
        assert_eq!(parse_reply_read("看不懂"), ReplyRead::default());
    }

    #[test]
    fn style_description_is_cleaned() {
        assert_eq!(clean_style("```\n口语化，短句为主\n```"), "口语化，短句为主");
        assert_eq!(clean_style("“口语化”").as_str(), "口语化");
        assert!(clean_style(&"风".repeat(500)).chars().count() <= 300);
        assert!(style_system().contains("120"));
    }
}
