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
    assert!(!out.contains("goodbye")); // 同一种格式的相邻文字块合成一个，不再留空壳
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
    assert_eq!(kind_of(Path::new("a.PPTX")), Some(Kind::Pptx));
    assert_eq!(kind_of(Path::new("a.xlsx")), Some(Kind::Xlsx));
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

/// 造一个 zip：条目名 → 内容。
fn make_zip(path: &Path, entries: &[(&str, &str)]) {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (n, c) in entries {
        w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

#[test]
fn office_files_load_every_text_part_and_write_back_keeping_formats() {
    let dir = std::env::temp_dir().join(format!("cc-filetr-office-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Word：正文、页眉、脚注都要翻译；正文里一段有粗体
    let docx = dir.join("a.docx");
    make_zip(
        &docx,
        &[
            ("word/document.xml", r#"<w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Warning: </w:t></w:r><w:r><w:t>hot surface</w:t></w:r></w:p></w:body>"#),
            ("word/header1.xml", r#"<w:hdr><w:p><w:r><w:t>Company manual</w:t></w:r></w:p></w:hdr>"#),
            ("word/footnotes.xml", r#"<w:footnotes><w:footnote><w:p><w:r><w:t>See appendix</w:t></w:r></w:p></w:footnote></w:footnotes>"#),
            ("word/media/image1.png", "PNG"),
        ],
    );
    let parts = load(&docx, Kind::Docx).unwrap();
    assert_eq!(parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["word/document.xml", "word/footnotes.xml", "word/header1.xml"]);
    assert_eq!(parts[0].slots[0].text, "<r1>Warning: </r1><r2>hot surface</r2>");
    let out = render_part(Kind::Docx, &parts[0], &tr(&["<r1>警告：</r1><r2>表面很烫</r2>"]), 0, false);
    assert!(out.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">警告：</w:t>") && out.contains("表面很烫"), "{out}");
    // PowerPoint：幻灯片按数字顺序、备注也翻译
    let pptx = dir.join("a.pptx");
    make_zip(
        &pptx,
        &[
            ("ppt/slides/slide10.xml", r#"<p:sld><a:p><a:r><a:rPr/><a:t>Tenth slide</a:t></a:r></a:p></p:sld>"#),
            ("ppt/slides/slide2.xml", r#"<p:sld><a:p><a:r><a:rPr/><a:t>Second slide</a:t></a:r></a:p></p:sld>"#),
            ("ppt/notesSlides/notesSlide2.xml", r#"<p:notes><a:p><a:r><a:rPr/><a:t>Speaker notes here</a:t></a:r></a:p></p:notes>"#),
        ],
    );
    let parts = load(&pptx, Kind::Pptx).unwrap();
    assert_eq!(
        parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["ppt/slides/slide2.xml", "ppt/slides/slide10.xml", "ppt/notesSlides/notesSlide2.xml"]
    );
    // Excel：共享字符串里的文字、网址不翻
    let xlsx = dir.join("a.xlsx");
    make_zip(
        &xlsx,
        &[
            ("xl/sharedStrings.xml", r#"<sst><si><t>Total revenue</t></si><si><t>https://example.com</t></si></sst>"#),
            ("xl/worksheets/sheet1.xml", "<worksheet/>"),
        ],
    );
    let parts = load(&xlsx, Kind::Xlsx).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].slots.len(), 1);
    let mut rendered = std::collections::HashMap::new();
    rendered.insert(parts[0].name.clone(), render_part(Kind::Xlsx, &parts[0], &tr(&["总收入"]), 0, false));
    let dst = dir.join("b.xlsx");
    write_zip(&xlsx, &dst, &rendered).unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(&dst).unwrap()).unwrap();
    let ss = read_zip_entry(&mut z, "xl/sharedStrings.xml").unwrap();
    assert!(ss.contains("总收入") && ss.contains("https://example.com"));
    assert_eq!(read_zip_entry(&mut z, "xl/worksheets/sheet1.xml").unwrap(), "<worksheet/>");
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
    let v =
        View {
            input: "x".into(), pages: vec![ViewPage { src: vec!["a".into()], trans: vec![Some("甲".into())], ..Default::default() }], ..Default::default()
        };
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

#[test]
fn images_and_scanned_pdf_are_recognised() {
    assert_eq!(kind_of(Path::new("a.PNG")), Some(Kind::Img));
    assert_eq!(kind_of(Path::new("a.jpeg")), Some(Kind::Img));
    assert_eq!(kind_of(Path::new("a.webp")), Some(Kind::Img));
    assert!(load(Path::new("a.png"), Kind::Img).is_err());
    // 2×1 的 RGB 像素能编成合法 PNG；数据不够就不编
    let png = png_of(2, 1, true, &[255, 0, 0, 0, 255, 0]).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert!(png_of(2, 1, true, &[1, 2, 3]).is_none());
    assert!(png_of(0, 0, false, &[]).is_none());
}

#[test]
fn ocr_engine_names_fall_back_to_the_vision_model() {
    let o = |s: &str| Opts { ocr: s.into(), ..Default::default() };
    assert_eq!(o("enhanced").engine(), "enhanced");
    assert_eq!(o("api").engine(), "api");
    assert_eq!(o("vision").engine(), "vision");
    assert_eq!(o("").engine(), "vision");
    assert_eq!(o("whatever").engine(), "vision");
    assert_eq!(Opts::default().engine(), "vision");
    // 旧的请求没有 ocr / preview 字段，照常能读
    let old: Opts = serde_json::from_str(r#"{"bilingual":true,"target":"","pdfMode":"inplace"}"#).unwrap();
    assert_eq!((old.engine(), old.preview), ("vision", false));
    let new: Opts = serde_json::from_str(r#"{"ocr":"api","preview":true}"#).unwrap();
    assert_eq!((new.engine(), new.preview), ("api", true));
}

#[test]
fn ocr_text_is_cached_per_engine() {
    let dir = Path::new("/data");
    let f = Path::new("/docs/scan.pdf");
    assert_ne!(ocr_path(dir, f, "vision"), ocr_path(dir, f, "enhanced"), "换了识字方式要重新识，不能用上次的结果");
    assert_eq!(ocr_path(dir, f, "api"), ocr_path(dir, f, "api"));
}

#[test]
fn images_decode_to_bgra() {
    // 2×1 的 PNG：红、绿
    let mut buf = vec![];
    {
        let mut e = png::Encoder::new(&mut buf, 2, 1);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[255, 0, 0, 0, 255, 0]).unwrap();
    }
    let (px, w, h) = decode_bgra(&buf).unwrap();
    assert_eq!((w, h), (2, 1));
    assert_eq!(px, vec![0, 0, 255, 255, 0, 255, 0, 255], "BGRA：红 = 0,0,255");
    assert!(decode_bgra(b"not an image").is_err());
}

#[test]
fn recognized_lines_become_paragraphs() {
    use crate::livecore::OLine;
    // 同一段的两行接起来，隔得远的另起一段
    let lines = vec![
        OLine::new("Dear customer,", 10.0, 10.0, 200.0, 20.0),
        OLine::new("thank you for your order.", 10.0, 34.0, 260.0, 20.0),
        OLine::new("Best regards", 10.0, 200.0, 140.0, 20.0),
    ];
    assert_eq!(lines_to_text(&lines), "Dear customer, thank you for your order.\n\nBest regards");
    assert_eq!(lines_to_text(&[]), "");
}

#[test]
fn pixel_boxes_map_to_pdf_coordinates() {
    // 图 1000×500 对应页 600×300 磅；像素框 (100,50)-(300,150)
    let (l, b, r, t) = pdf_rect((100.0, 50.0, 300.0, 150.0), (1000, 500), (600.0, 300.0));
    assert!((l - 60.0).abs() < 0.01 && (r - 180.0).abs() < 0.01);
    // PDF 的 y 轴向上：图上 y=50 在页上 = 300 - 30 = 270；y=150 → 210
    assert!((t - 270.0).abs() < 0.01 && (b - 210.0).abs() < 0.01);
}

#[test]
fn edits_in_the_reader_reach_the_cache() {
    let dir = std::env::temp_dir().join(format!("cc-filetr-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("a.txt");
    std::fs::write(&input, "Hello world.\n\nSecond paragraph.").unwrap();
    let cp = cache_path(&dir, &input, "简体中文");
    let v = View {
        input: input.to_string_lossy().into_owned(),
        target: "简体中文".into(),
        bilingual: true,
        pages: vec![ViewPage {
            src: vec!["Hello world.".into(), "Second paragraph.".into()],
            trans: vec![Some("你好，世界。".into()), None],
            ..Default::default()
        }],
        failed: 1,
        ..Default::default()
    };
    save_cache(&cp, &v);
    // 改第二段：failed 跟着变
    edit_seg(&dir, &input, "简体中文", 0, 1, "  第二段。 ").unwrap();
    let got = view_for(&dir, &input, "简体中文").unwrap();
    assert_eq!(got.pages[0].trans, vec![Some("你好，世界。".to_string()), Some("第二段。".to_string())]);
    assert_eq!(got.failed, 0);
    // 清空 = 去掉译文
    edit_seg(&dir, &input, "简体中文", 0, 0, "   ").unwrap();
    let got = view_for(&dir, &input, "简体中文").unwrap();
    assert_eq!(got.pages[0].trans[0], None);
    assert_eq!(got.failed, 1);
    // 越界 / 没有缓存都报错，不会 panic
    assert!(edit_seg(&dir, &input, "简体中文", 3, 0, "x").is_err());
    assert!(edit_seg(&dir, &input, "English", 0, 0, "x").is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_blocks_round_trip_through_the_sidecar() {
    let pages = vec![
        vec![ScanBlock { src: "Hi".into(), trans: "你好".into(), rect: Some((1.0, 2.0, 3.0, 4.0)), size: 9.5, rgb: [0, 0, 0], bg: Some([255, 255, 255]) }],
        vec![],
    ];
    let back: Vec<Vec<ScanBlock>> = serde_json::from_slice(&serde_json::to_vec(&pages).unwrap()).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0][0].trans, "你好");
    assert_eq!(back[0][0].rect, Some((1.0, 2.0, 3.0, 4.0)));
    // 译文不同的识字方式不共用旁路文件
    let (d, f) = (Path::new("/d"), Path::new("/f.pdf"));
    assert_ne!(scan_path(d, f, "简体中文", "vision"), scan_path(d, f, "简体中文", "enhanced"));
    assert_ne!(scan_path(d, f, "简体中文", "vision"), scan_path(d, f, "English", "vision"));
}

#[test]
fn background_is_sampled_around_the_block_with_the_pdf_y_axis_flipped() {
    // 页面 100×200 点，渲染成 100×200 像素：上半白、下半深蓝；左下角的块（PDF 坐标 y 朝上）应取到深蓝，上方的块取到白
    let (w, h) = (100usize, 200usize);
    let mut rgba = vec![255u8; w * h * 4];
    for y in 100..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[10, 20, 120, 255]);
        }
    }
    let bottom_block = (10.0, 20.0, 60.0, 50.0); // PDF 点：靠近页面底部 → 图片下半
    let top_block = (10.0, 150.0, 60.0, 180.0); // 靠近页面顶部 → 图片上半
    assert_eq!(sample_rect_bg(&rgba, w, h, (100.0, 200.0), bottom_block), [10, 20, 120]);
    assert_eq!(sample_rect_bg(&rgba, w, h, (100.0, 200.0), top_block), [255, 255, 255]);
    // 框超出页面也不会越界
    assert_eq!(sample_rect_bg(&rgba, w, h, (100.0, 200.0), (-5.0, -5.0, 500.0, 500.0)).len(), 3);
}

// ---------- 版面：按页对上缓存（译到一半 / 停止 / 失败 / 换了文件 / 旧缓存都不能整页报错） ----------

fn block(text: &str) -> crate::pdfx::Block {
    crate::pdfx::Block { runs: vec![0], l: 0.0, b: 0.0, r: 10.0, t: 10.0, size: 10.0, rgb: [0, 0, 0], text: text.into(), lines: 1 }
}
fn pdf_page(texts: &[&str]) -> crate::pdfx::PageInfo {
    crate::pdfx::PageInfo { w: 100.0, h: 100.0, blocks: texts.iter().map(|t| block(t)).collect(), has_text: !texts.is_empty() }
}
fn vp(page: usize, src: &[&str], trans: &[Option<&str>], done: bool) -> ViewPage {
    ViewPage {
        src: src.iter().map(|s| s.to_string()).collect(),
        trans: trans.iter().map(|t| t.map(str::to_string)).collect(),
        page: Some(page),
        done,
        skip: vec![],
        adj: Default::default(),
        qa: Default::default(),
    }
}

#[test]
fn layout_page_matches_even_when_other_pages_are_not_translated_yet() {
    // 3 页，只译了第 0 页（译到一半停止 / 正在翻译）：第 0 页有译文，第 2 页没有，都不报错
    let pages = vec![pdf_page(&["Hello", "World"]), pdf_page(&[]), pdf_page(&["Third page text"])];
    let mut view = Some(View {
        v: VIEW_V,
        layout: true,
        pages: vec![vp(0, &["Hello", "World"], &[Some("你好"), Some("世界")], true), vp(2, &["Third page text"], &[None], false)],
        ..Default::default()
    });
    let m = match_cached_page(&mut view, &pages, 0);
    assert_eq!((m.trans, m.done, m.view_page, m.dirty), (vec![Some("你好".to_string()), Some("世界".to_string())], true, 0, false));
    let m = match_cached_page(&mut view, &pages, 2);
    assert_eq!((m.trans, m.done, m.view_page, m.dirty), (vec![None], false, 1, false), "还没译的页：未译，页没处理完");
}

#[test]
fn layout_page_without_an_entry_gets_a_slot_in_page_order() {
    let pages = vec![pdf_page(&["a"]), pdf_page(&["b"]), pdf_page(&["c"])];
    let mut view =
        Some(View { v: VIEW_V, layout: true, pages: vec![vp(0, &["a"], &[Some("甲")], true), vp(2, &["c"], &[None], false)], ..Default::default() });
    let m = match_cached_page(&mut view, &pages, 1);
    assert!(m.dirty && m.view_page == 1 && m.trans == vec![None::<String>]);
    let v = view.unwrap();
    assert_eq!(v.pages.iter().map(|p| p.page).collect::<Vec<_>>(), vec![Some(0), Some(1), Some(2)], "按页号排好，后面生成文件时的顺序才对");
}

#[test]
fn layout_page_is_rematched_by_text_when_the_blocks_changed() {
    // 换了排版方式：这一页现在是 3 块（原来 2 块），“Hello”还在：照用，新的两块是未译；缓存改成当前的块
    let pages = vec![pdf_page(&["Hello", "Brand new", "World!"])];
    let mut view =
        Some(View { v: VIEW_V, layout: true, pages: vec![vp(0, &["Hello", "World"], &[Some("你好"), Some("世界")], true)], ..Default::default() });
    let m = match_cached_page(&mut view, &pages, 0);
    assert_eq!(m.trans, vec![Some("你好".to_string()), None, None]);
    assert!(m.dirty && m.note.contains("重新匹配") && m.note.contains("2 段"), "{}", m.note);
    let v = view.unwrap();
    assert_eq!(v.pages[0].src, vec!["Hello", "Brand new", "World!"], "缓存改成了当前的块：之后在页面上改译文，序号对得上");
    // 再取一次：直接对上，不再提示
    let mut view = Some(v);
    let m = match_cached_page(&mut view, &pages, 0);
    assert!(!m.dirty && m.note.is_empty());
}

#[test]
fn skipped_blocks_are_not_reported_as_untranslated() {
    let pages = vec![pdf_page(&["Header", "Body"])];
    let mut p = vp(0, &["Header", "Body"], &[None, Some("正文")], true);
    p.skip = vec![0];
    let mut view = Some(View { v: VIEW_V, layout: true, pages: vec![p], ..Default::default() });
    let m = match_cached_page(&mut view, &pages, 0);
    assert_eq!(m.skip, vec![0]);
}

#[test]
fn legacy_caches_still_work_when_the_totals_match_and_say_why_when_not() {
    let pages = vec![pdf_page(&["a", "b"]), pdf_page(&["c"])];
    // 旧缓存：没有页号，按“所有页的所有译文排成一串”
    let legacy =
        |n: usize| View { pages: vec![ViewPage { src: vec!["x".into(); n], trans: vec![Some("译".into()); n], ..Default::default() }], ..Default::default() };
    let mut view = Some(legacy(3));
    let m = match_cached_page(&mut view, &pages, 1);
    assert_eq!((m.trans, m.done, m.view_page), (vec![Some("译".to_string())], true, 1));
    let mut view = Some(legacy(5)); // 当时是按段落切的
    let m = match_cached_page(&mut view, &pages, 1);
    assert!(m.note.contains("按段落切") && m.trans == vec![None::<String>] && !m.dirty);
    // 完全没有缓存
    let mut none: Option<View> = None;
    let m = match_cached_page(&mut none, &pages, 0);
    assert!(m.note.contains("还没有译文") && m.trans.len() == 2);
}

#[test]
fn old_caches_are_marked_done_on_load_and_failed_pages_are_listed() {
    let dir = std::env::temp_dir().join(format!("cc-filetr-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cp = dir.join("old.json");
    // 没有 v / layout / page / done 字段的旧缓存
    std::fs::write(
        &cp,
        r#"{"input":"a.txt","target":"简体中文","bilingual":true,"pages":[{"src":["a"],"trans":["甲"]},{"src":["b","c"],"trans":[null,"丙"]}],"failed":1}"#,
    )
    .unwrap();
    let v = load_cache(&cp).unwrap();
    assert!(v.pages.iter().all(|p| p.done), "旧缓存每页都是译完才存的");
    assert_eq!(failed_pages_text(&v), "第 2 页 1 段");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manual_block_adjustments_override_the_automatic_layout_and_survive_the_cache_format() {
    let a = BlockAdj { rect: Some([10.0, 20.0, 110.0, 60.0]), size: Some(9.5) };
    assert_eq!(a.apply((0.0, 0.0, 50.0, 50.0), 12.0), ((10.0, 20.0, 110.0, 60.0), 9.5));
    // 只改了字号：位置沿用；字号小于 3 当作没写
    assert_eq!(BlockAdj { rect: None, size: Some(14.0) }.apply((1.0, 2.0, 3.0, 4.0), 8.0), ((1.0, 2.0, 3.0, 4.0), 14.0));
    assert_eq!(BlockAdj { rect: None, size: Some(1.0) }.apply((1.0, 2.0, 3.0, 4.0), 8.0).1, 8.0);
    // 缓存里按“序号 → 调整”存，读回来一样；没有这个字段的旧缓存照常读
    let mut p = ViewPage::default();
    p.adj.insert(3, a);
    let back: ViewPage = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back.adj.get(&3), Some(&a));
    let old: ViewPage = serde_json::from_str(r#"{"src":["x"],"trans":[null]}"#).unwrap();
    assert!(old.adj.is_empty());
}
