//! 后端少量面向用户的固定文字（托盘菜单、托盘提示、系统通知标题）的英文翻译。
//! 界面里的文字由前端翻译（见 `src/i18n`）；这里只放前端够不着的原生菜单和通知。
//! 动态内容（作品标题、错误消息）不翻译。

/// 界面语言是不是英文。
pub fn is_en(language: &str) -> bool {
    language == "en"
}

const TABLE: &[(&str, &str)] = &[
    ("显示主窗口", "Show main window"),
    ("解析剪贴板", "Parse clipboard"),
    ("监听剪贴板", "Watch clipboard"),
    ("锁定", "Lock"),
    ("退出清影", "Quit ClearClip"),
    ("下载完成", "Download complete"),
    ("详情请打开清影查看", "Open ClearClip for details"),
    ("清影", "ClearClip"),
    ("有新消息", "New activity"),
];

/// 取固定文字的译文；没有对应条目或不是英文时原样返回。
pub fn tr(en: bool, zh: &str) -> String {
    if en {
        if let Some((_, e)) = TABLE.iter().find(|(z, _)| *z == zh) {
            return (*e).to_string();
        }
    }
    zh.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_strings_only_in_english() {
        assert_eq!(tr(true, "退出清影"), "Quit ClearClip");
        assert_eq!(tr(false, "退出清影"), "退出清影");
        assert_eq!(tr(true, "没有这一条"), "没有这一条");
        assert!(is_en("en") && !is_en("zh") && !is_en("ja"));
    }
}
