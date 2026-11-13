//! 给 AI 之前的字幕文字预处理：非语音说明（`[Music]` `♪` `>>` 说话人前缀）和口水词 / 不流畅（嗯、啊、um、结巴、叠词）。
//!
//! 只作用在“送给模型的文字”上（摘要、导图、卡片、金句、问答、补标点、字幕翻译），**不改文稿的显示和导出**：
//! 精听和学语言要原话，摘要和翻译要干净的文字。标记、实体这类结构问题在 `subtitle::clean_text` 里处理（始终执行）。
//!
//! 口水词分四档：关 / 保守（只去没有歧义的语气词、结巴）/ 标准（再加成串的口头禅）/ 激进（再加被逗号隔开的口头禅）。
//! 清理后文字变空的行：翻译保留原文（要和显示的行一一对应），其余类型直接丢掉。

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off,
    Conservative,
    Standard,
    Aggressive,
}

pub fn parse_level(s: &str) -> Level {
    match s.trim() {
        "off" | "none" => Level::Off,
        "standard" => Level::Standard,
        "aggressive" => Level::Aggressive,
        _ => Level::Conservative,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Opts {
    /// 去掉非语音说明
    pub hearing: bool,
    pub filler: Level,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("subclean regex")
}

// ---------- 非语音说明 ----------
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| re(r"\[[^\[\]]{0,40}\]|【[^【】]{0,40}】|［[^［］]{0,40}］"));
/// 圆括号里只有这些词才算说明（`(1)` `(see below)` 这类是正文）
static PAREN_NOTE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)[\(（]\s*(?:laugh(?:s|ter|ing)?|applause|applauding|cheer(?:s|ing)?|music|singing|sighs?|sniffs?|coughs?|clears throat|gasps?|crying|sobbing|inaudible|silence|whispers?|chuckles?|groans?|screams?|laughs softly|笑|笑声|掌声|音乐|歌声|叹气|咳嗽|哭|无声|静音|鼓掌)\s*[\)）]")
});
static MUSIC_SYMBOLS: LazyLock<Regex> = LazyLock::new(|| re(r"[♪♫♬♩🎵🎶]+"));
static SPEAKER_ARROWS: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:>>+|»+|&gt;&gt;)\s*"));
static SPEAKER_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Z][A-Z0-9 .'\-]{1,24}:\s+"));
static DIALOGUE_DASH: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*[-–—]\s+"));

fn strip_hearing(s: &str) -> String {
    let t = BRACKETED.replace_all(s, " ");
    let t = PAREN_NOTE.replace_all(&t, " ");
    let t = MUSIC_SYMBOLS.replace_all(&t, " ");
    let t = SPEAKER_ARROWS.replace(&t, "");
    let t = SPEAKER_NAME.replace(&t, "");
    DIALOGUE_DASH.replace(&t, "").into_owned()
}

// ---------- 口水词 ----------
// 保守：没有歧义的语气词
static EN_UM: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:u+m+|u+h+m*|e+r+m*|h+m+|m{2,}|a+h{2,})\b[,.…\-]*\s*"));
static ZH_LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:[嗯呃额唔]+[，,、。.…\s]*)+"));
static ZH_TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"[，,、\s]*[嗯呃额唔]+[。.…]*$"));
static ZH_MID: LazyLock<Regex> = LazyLock::new(|| re(r"[，,、][嗯呃额唔]+[，,、]"));
static JA_FILLER: LazyLock<Regex> = LazyLock::new(|| re(r"(?:え{1,3}ー+|えっと|ええと|あのー+|そのー+|うーん|んー+)[、。…]*\s*"));
static KO_FILLER: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|\s)(?:음{1,3}|으음|어{2,})(?:[,.…]|\s|$)"));
static STUTTER_HYPHEN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b[a-z]{1,3}-(?:[a-z]{1,3}-)*([a-z]+)\b"));
// 标准：被逗号隔开的口头禅
static EN_PHRASE_LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(^|[,.!?]\s+)(?:you know|i mean|sort of|kind of like)\s*,\s*"));
static EN_PHRASE_MID: LazyLock<Regex> = LazyLock::new(|| re(r"(?i),\s*(?:you know|i mean)\s*,"));
static EN_PHRASE_TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i),\s*(?:you know|i mean)\s*([.!?])"));
static ZH_PARTICLE_LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^[啊哦噢哎]+[，,。.…\s]+"));
// 激进：被逗号隔开的单个口头禅
static EN_LIKE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i),\s*(?:like|basically|actually|literally)\s*,"));
static ZH_TICS: LazyLock<Regex> = LazyLock::new(|| re(r"[，,]\s*(?:那个|就是|然后|这个|对吧|你知道)\s*[，,]"));

/// 可以安全叠用的短词：连续重复只留一个（`I I think` → `I think`）。`had had`、`that that` 这类真的可以重复，不在表里。
const STUTTER_WORDS: &[&str] = &["i", "the", "a", "an", "to", "and", "it", "we", "you", "they", "he", "she", "is", "in", "of", "so", "but", "my", "your", "our", "that's", "i'm", "it's"];

fn collapse_en_repeats(s: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for w in s.split_whitespace() {
        let key = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
        if let Some(prev) = out.last() {
            let pk = prev.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
            if !key.is_empty() && key == pk && STUTTER_WORDS.contains(&key.as_str()) {
                continue;
            }
        }
        out.push(w);
    }
    out.join(" ")
}

/// 汉字叠词：`对对对对` → `对`（只处理表里的、说话时常连着重复的字）；`unit_words` 为真时也处理成串的口头禅（`那个那个` → `那个`）。
fn collapse_zh_repeats(s: &str, unit_words: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '对' | '是' | '好' | '行' | '嗯' | '哈' | '啊') && i + 2 < chars.len() && chars[i + 1] == c && chars[i + 2] == c {
            // 三个以上：留一个（“哈哈哈”是笑，不处理）
            if c == '哈' {
                out.push(c);
                i += 1;
                continue;
            }
            let mut j = i;
            while j < chars.len() && chars[j] == c {
                j += 1;
            }
            out.push(c);
            i = j;
            continue;
        }
        if unit_words && i + 3 < chars.len() {
            let pair: String = chars[i..i + 2].iter().collect();
            if matches!(pair.as_str(), "那个" | "就是" | "这个" | "然后") {
                let mut j = i + 2;
                while j + 1 < chars.len() && chars[j..j + 2].iter().collect::<String>() == pair {
                    j += 2;
                }
                if j > i + 2 {
                    out.push_str(&pair);
                    i = j;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn strip_fillers(s: &str, level: Level) -> String {
    if level == Level::Off {
        return s.to_string();
    }
    let mut t = s.to_string();
    t = EN_UM.replace_all(&t, "").into_owned();
    t = ZH_LEAD.replace(&t, "").into_owned();
    t = ZH_TAIL.replace(&t, "").into_owned();
    t = ZH_MID.replace_all(&t, "，").into_owned();
    t = JA_FILLER.replace_all(&t, "").into_owned();
    t = KO_FILLER.replace_all(&t, " ").into_owned();
    t = STUTTER_HYPHEN.replace_all(&t, "$1").into_owned();
    t = collapse_en_repeats(&t);
    t = collapse_zh_repeats(&t, level >= Level::Standard);
    if level >= Level::Standard {
        t = EN_PHRASE_MID.replace_all(&t, " ").into_owned();
        t = EN_PHRASE_TAIL.replace_all(&t, "$1").into_owned();
        t = EN_PHRASE_LEAD.replace_all(&t, "$1").into_owned();
        t = ZH_PARTICLE_LEAD.replace(&t, "").into_owned();
    }
    if level >= Level::Aggressive {
        t = EN_LIKE.replace_all(&t, ",").into_owned();
        t = ZH_TICS.replace_all(&t, "，").into_owned();
    }
    t
}

fn has_content(s: &str) -> bool {
    s.chars().any(|c| c.is_alphanumeric())
}

/// 清理一行；变成没有内容（只剩标点）时返回空串。
pub fn clean_line(text: &str, o: &Opts) -> String {
    let mut t = text.to_string();
    if o.hearing {
        t = strip_hearing(&t);
    }
    t = strip_fillers(&t, o.filler);
    // 去掉清理后留下的孤立标点和多余空白：行首的逗号句号、连续空格、逗号前的空格
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_start_matches(|c: char| matches!(c, ',' | '，' | '、' | '.' | ';' | '；')).trim().to_string();
    if has_content(&t) {
        t
    } else {
        String::new()
    }
}

/// 请求里的字幕行：把要送给模型的文字清理一遍。`keep_empty` 为真（翻译）时，清空的行保留原文，保证和显示的行一一对应；否则清空的行变成空串（后面的流程会跳过）。
pub fn prepare_lines(lines: &mut [crate::vsum::Line], o: &Opts, keep_empty: bool) {
    if !o.hearing && o.filler == Level::Off {
        return;
    }
    for l in lines.iter_mut() {
        let cleaned = clean_line(&l.text, o);
        if cleaned.is_empty() {
            if !keep_empty {
                l.text.clear();
            }
        } else {
            l.text = cleaned;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(level: Level) -> Opts {
        Opts { hearing: true, filler: level }
    }

    #[test]
    fn hearing_notes_and_speaker_marks() {
        let c = |s: &str| clean_line(s, &o(Level::Off));
        assert_eq!(c("[Music]"), "");
        assert_eq!(c("♪ ♪ la la ♪"), "la la");
        assert_eq!(c("Thanks [Applause] everyone"), "Thanks everyone");
        assert_eq!(c("(laughs) That was good"), "That was good");
        assert_eq!(c("这是[音乐]一首歌"), "这是 一首歌".replace(' ', " "));
        assert_eq!(c(">> So we begin"), "So we begin");
        assert_eq!(c("JOHN: hello there"), "hello there");
        assert_eq!(c("you [ __ ] idiot"), "you idiot");
        // 圆括号里不是说明的，是正文
        assert_eq!(c("the answer (1) is yes"), "the answer (1) is yes");
        assert_eq!(c("(see below)"), "(see below)");
    }

    #[test]
    fn conservative_fillers_only_remove_unambiguous_ones() {
        let c = |s: &str| clean_line(s, &o(Level::Conservative));
        assert_eq!(c("Um, so I think uh we should go"), "so I think we should go");
        assert_eq!(c("Hmm... let me see"), "let me see");
        assert_eq!(c("I I think the the plan works"), "I think the plan works");
        // 真的可以重复的词不动
        assert_eq!(c("he said that that is fine"), "he said that that is fine");
        assert_eq!(c("It had had no effect"), "It had had no effect");
        assert_eq!(c("w-w-what is that"), "what is that");
        // 有歧义的口头禅保守档不碰
        assert_eq!(c("It was, you know, fine and like really big"), "It was, you know, fine and like really big");
        assert_eq!(c("嗯，我觉得可以"), "我觉得可以");
        assert_eq!(c("这个方案，呃，可以"), "这个方案，可以");
        assert_eq!(c("好的嗯"), "好的");
        assert_eq!(c("对对对，没错"), "对，没错");
        assert_eq!(c("哈哈哈"), "哈哈哈");
        assert_eq!(c("嗯"), "");
        assert_eq!(c("えっと、今日は晴れです"), "今日は晴れです");
        // 那个 / 就是 保守档不动
        assert_eq!(c("那个人就是他"), "那个人就是他");
    }

    #[test]
    fn standard_and_aggressive_levels() {
        let s = |t: &str| clean_line(t, &o(Level::Standard));
        let a = |t: &str| clean_line(t, &o(Level::Aggressive));
        assert_eq!(s("We went, you know, to the park."), "We went to the park.");
        assert_eq!(s("You know, it works"), "it works");
        assert_eq!(s("那个那个我们开始吧"), "那个我们开始吧");
        assert_eq!(s("啊，好的"), "好的");
        // 标准档不处理单个“那个”“就是”
        assert_eq!(s("我们，就是，开始"), "我们，就是，开始");
        assert_eq!(a("我们，就是，开始"), "我们，开始");
        assert_eq!(a("It was, like, huge"), "It was, huge");
    }

    #[test]
    fn off_keeps_everything_and_empty_lines_follow_the_flag() {
        assert_eq!(clean_line("Um, hello", &Opts { hearing: false, filler: Level::Off }), "Um, hello");
        let mut v = vec![crate::vsum::Line { t: 0, text: "嗯".into() }, crate::vsum::Line { t: 1, text: "好的".into() }];
        prepare_lines(&mut v, &o(Level::Conservative), true);
        assert_eq!(v[0].text, "嗯", "翻译要和显示的行一一对应，清空的保留原文");
        let mut v2 = vec![crate::vsum::Line { t: 0, text: "嗯".into() }];
        prepare_lines(&mut v2, &o(Level::Conservative), false);
        assert_eq!(v2[0].text, "");
    }
}
