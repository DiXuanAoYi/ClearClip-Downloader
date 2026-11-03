use super::*;

fn part(kind: Kind, raw: &str) -> Part {
    let slots = match kind {
        Kind::Txt => extract_plain(raw, false),
        Kind::Md => extract_plain(raw, true),
        Kind::Docx => extract_docx(raw),
        _ => extract_html(raw),
    };
    Part { name: String::new(), raw: raw.to_string(), slots }
}

fn tr(v: &[&str]) -> Vec<Option<String>> {
    v.iter().map(|s| Some(s.to_string())).collect()
}

#[test]
fn txt_paragraphs() {
    let p = part(Kind::Txt, "Hello world.\nSecond line.\n\n123\n\nBye.\n");
    // 纯数字不翻译；硬换行的段落合成一段
    assert_eq!(p.slots.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["Hello world. Second line.", "Bye."]);
    assert_eq!(render_part(Kind::Txt, &p, &tr(&["你好。", "再见。"]), 0, false), "你好。\n\n123\n\n再见。\n");
    assert_eq!(render_part(Kind::Txt, &p, &tr(&["你好。", "再见。"]), 0, true), "Hello world.\nSecond line.\n你好。\n\n123\n\nBye.\n再见。\n");
}

#[test]
fn markdown_keeps_structure() {
    let raw = "# Title\n\nSome text here.\n\n```rust\nlet x = 1;\n```\n\n- item one\n- item two\n\n> quoted words\n\n---\n";
    let p = part(Kind::Md, raw);
    let texts: Vec<_> = p.slots.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["Title", "Some text here.", "item one", "item two", "quoted words"]);
    let out = render_part(Kind::Md, &p, &tr(&["标题", "一些文字。", "条目一", "条目二", "引用"]), 0, false);
    assert_eq!(out, "# 标题\n\n一些文字。\n\n```rust\nlet x = 1;\n```\n\n- 条目一\n- 条目二\n\n> 引用\n\n---\n");
}

#[test]
fn docx_paragraph_text_is_replaced_in_place() {
    let raw = r#"<w:body><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>Hello </w:t></w:r><w:r><w:t>&amp; goodbye</w:t></w:r></w:p><w:p><w:r><w:t>42</w:t></w:r></w:p></w:body>"#;
    let p = part(Kind::Docx, raw);
    assert_eq!(p.slots.len(), 1);
    assert_eq!(p.slots[0].text, "Hello & goodbye");
    let out = render_part(Kind::Docx, &p, &tr(&["你好 & 再见"]), 0, false);
    assert!(out.contains(r#"<w:jc w:val="center"/>"#));
    assert!(out.contains(r#"<w:t xml:space="preserve">你好 &amp; 再见</w:t>"#));
    assert!(out.contains("<w:t></w:t>"));
    assert!(out.contains("<w:t>42</w:t>"));
    let bi = render_part(Kind::Docx, &p, &tr(&["你好"]), 0, true);
    assert_eq!(bi.matches("<w:p>").count(), 3);
    assert!(bi.contains("Hello "));
}

#[test]
fn epub_blocks_nested_and_entities() {
    let raw = "<body><h1 id=\"a\">Chapter&nbsp;One</h1><p>It&#8217;s <i>fine</i>.<br/>Really.</p><ul><li>one<ul><li>nested</li></ul></li><li><p>para in li</p></li></ul><pre>code</pre><p>3.14</p></body>";
    let p = part(Kind::Epub, raw);
    let texts: Vec<_> = p.slots.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["Chapter One", "It’s fine. Really.", "nested", "para in li"]);
    let out = render_part(Kind::Epub, &p, &tr(&["第一章", "没关系。真的。", "嵌套", "段落"]), 0, false);
    assert!(out.starts_with("<body><h1 id=\"a\">第一章</h1><p>没关系。真的。</p>"));
    assert!(out.contains("<li>one<ul><li>嵌套</li></ul></li>"));
    assert!(out.contains("<li><p>段落</p></li>"));
    assert!(out.contains("<pre>code</pre><p>3.14</p>"));
}

#[test]
fn missing_translation_keeps_original() {
    let p = part(Kind::Txt, "One.\n\nTwo.\n");
    let t = vec![Some("一。".to_string()), None];
    assert_eq!(render_part(Kind::Txt, &p, &t, 0, false), "一。\n\nTwo.\n");
}

#[test]
fn kinds() {
    assert_eq!(kind_of(Path::new("a.EPUB")), Some(Kind::Epub));
    assert_eq!(kind_of(Path::new("a.srt")), Some(Kind::Sub));
    assert_eq!(kind_of(Path::new("a.exe")), None);
}

#[test]
fn epub_zip_roundtrip_keeps_other_entries() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("cc-filetr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("book.epub");
    {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&src).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let def = zip::write::SimpleFileOptions::default();
        w.start_file("mimetype", stored).unwrap();
        w.write_all(b"application/epub+zip").unwrap();
        w.start_file("OEBPS/ch1.xhtml", def).unwrap();
        w.write_all(b"<html><body><p>Hello there.</p></body></html>").unwrap();
        w.start_file("OEBPS/cover.jpg", def).unwrap();
        w.write_all(&[0xff, 0xd8, 0x00, 0x01]).unwrap();
        w.finish().unwrap();
    }
    let parts = load(&src, Kind::Epub).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "OEBPS/ch1.xhtml");
    let mut rendered = std::collections::HashMap::new();
    rendered.insert(parts[0].name.clone(), render_part(Kind::Epub, &parts[0], &tr(&["你好。"]), 0, true));
    let dst = dir.join("book.双语.epub");
    write_zip(&src, &dst, &rendered).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(&dst).unwrap()).unwrap();
    assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
    let ch = read_zip_entry(&mut z, "OEBPS/ch1.xhtml").unwrap();
    assert_eq!(ch, "<html><body><p>Hello there.</p>\n<p>你好。</p></body></html>");
    let mut jpg = vec![];
    z.by_name("OEBPS/cover.jpg").unwrap().read_to_end(&mut jpg).unwrap();
    assert_eq!(jpg, [0xff, 0xd8, 0x00, 0x01]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pdf_lines_are_reflowed_into_paragraphs() {
    let pages = vec!["This is a long sen-\ntence that wraps.\nNext para starts\nhere and ends.\n".to_string(), "中文第一行\n接着第二行。\n".to_string()];
    let t = reflow_pdf(&pages);
    assert_eq!(t, "This is a long sentence that wraps.\n\nNext para starts here and ends.\n\n中文第一行接着第二行。\n\n");
    assert_eq!(extract_plain(&t, false).len(), 3);
    assert_eq!(kind_of(Path::new("a.pdf")), Some(Kind::Pdf));
}

#[test]
fn page_sizes_cover_everything() {
    // 字幕：每 40 条一页，最后一页是余数
    assert_eq!(page_sizes(&[], 95, true), vec![40, 40, 15]);
    assert_eq!(page_sizes(&[], 0, true), vec![0]);
    // 文档：按各个部分（章 / 页）原样分
    assert_eq!(page_sizes(&[3, 0, 5], 8, false), vec![3, 0, 5]);
}

#[test]
fn cache_roundtrip_and_key_depends_on_target() {
    let dir = std::env::temp_dir().join(format!("cc-filetr-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.txt");
    std::fs::write(&f, "hello").unwrap();
    let a = cache_path(&dir, &f, "简体中文");
    assert_ne!(a, cache_path(&dir, &f, "English"));
    let v = View { input: "x".into(), pages: vec![ViewPage { src: vec!["a".into()], trans: vec![Some("甲".into())] }], ..Default::default() };
    save_cache(&a, &v);
    let back = load_cache(&a).unwrap();
    assert_eq!(back.pages[0].trans[0].as_deref(), Some("甲"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn preview_splits_text_into_pages() {
    let dir = std::env::temp_dir().join(format!("cc-filetr-pv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("b.md");
    std::fs::write(&f, "# Title\n\nFirst paragraph here.\n\nSecond paragraph here.\n").unwrap();
    let v = preview(&f).unwrap();
    assert_eq!(v.pages.len(), 1);
    assert!(v.pages[0].src.len() >= 2);
    assert!(v.pages[0].trans.iter().all(Option::is_none));
    let _ = std::fs::remove_dir_all(&dir);
}
