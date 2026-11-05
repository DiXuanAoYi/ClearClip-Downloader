use super::*;

fn run(idx: usize, text: &str, l: f32, b: f32, r: f32, size: f32) -> Run {
    Run { idx, text: text.into(), l, b, r, t: b + size, size, rgb: [0, 0, 0] }
}

#[test]
fn lines_of_one_paragraph_become_one_block_and_a_heading_stays_apart() {
    // 页面坐标：原点在左下，所以 y 越大越靠上
    let runs = vec![
        run(0, "3.2 Attention", 72.0, 700.0, 160.0, 14.0),
        // 段落：三行，每行由两个文字对象拼成
        run(1, "An attention function can be described as", 72.0, 670.0, 270.0, 10.0),
        run(2, "mapping a query and a set", 273.0, 670.0, 400.0, 10.0),
        run(3, "of key-value pairs to an output, where the", 72.0, 657.0, 290.0, 10.0),
        run(4, "query, keys, values, and output", 293.0, 657.0, 420.0, 10.0),
        run(5, "are all vectors.", 72.0, 644.0, 150.0, 10.0),
        // 页码
        run(6, "3", 300.0, 40.0, 306.0, 9.0),
    ];
    let blocks = group_blocks(&runs);
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert_eq!(blocks[0].text, "3.2 Attention");
    assert_eq!(blocks[1].lines, 3);
    assert_eq!(blocks[1].text, "An attention function can be described as mapping a query and a set of key-value pairs to an output, where the query, keys, values, and output are all vectors.");
    assert_eq!(blocks[1].runs.len(), 5, "页码不在块里，也不会被删");
    assert_eq!((blocks[1].l, blocks[1].r), (72.0, 420.0));
}

#[test]
fn two_columns_do_not_merge_and_a_finished_short_line_ends_the_paragraph() {
    let runs = vec![
        run(0, "Left column first line of text here", 72.0, 700.0, 260.0, 10.0),
        run(1, "Left column second line of text", 72.0, 687.0, 250.0, 10.0),
        run(2, "Right column line one of the text", 320.0, 700.0, 510.0, 10.0),
        run(3, "Right column line two of the text", 320.0, 687.0, 505.0, 10.0),
        // 左栏第一段到此结束（句号 + 偏短），下一行是新段落
        run(4, "Short end.", 72.0, 674.0, 118.0, 10.0),
        run(5, "A new paragraph begins right here.", 72.0, 661.0, 255.0, 10.0),
    ];
    let b = group_blocks(&runs);
    let texts: Vec<&str> = b.iter().map(|x| x.text.as_str()).collect();
    assert!(texts.contains(&"Left column first line of text here Left column second line of text Short end."), "{texts:?}");
    assert!(texts.contains(&"Right column line one of the text Right column line two of the text"), "{texts:?}");
    assert!(texts.contains(&"A new paragraph begins right here."), "{texts:?}");
    assert_eq!(b.len(), 3);
}

#[test]
fn chinese_lines_join_without_spaces_and_hyphens_are_removed() {
    assert_eq!(join_lines("注意力函数可以描述为", "把一个查询映射成输出"), "注意力函数可以描述为把一个查询映射成输出");
    assert_eq!(join_lines("high-dimen-", "sional space"), "high-dimen-sional space".replace("dimen-sional", "dimensional"));
    assert_eq!(join_lines("state-of-the-art", "results"), "state-of-the-art results");
    assert_eq!(join_lines("Hello", "world"), "Hello world");
    assert_eq!(join_lines("", "x"), "x");
    assert!(worth("Attention"));
    assert!(worth("注意力"));
    assert!(!worth("12 34 56"));
    assert!(!worth("3"));
}

#[test]
fn wrapping_respects_width_punctuation_and_long_words() {
    // 中文每字一个字宽：宽 50、字号 10 → 每行 5 个字
    let l = wrap("今天天气真的不错，我们出去走走吧。", 50.0, 10.0);
    assert!(l.iter().all(|x| text_w(x, 10.0) <= 50.5), "{l:?}");
    assert!(l.iter().skip(1).all(|x| !x.starts_with('，') && !x.starts_with('。')), "标点不在行首：{l:?}");
    assert_eq!(l.concat(), "今天天气真的不错，我们出去走走吧。");
    // 英文在空格处断
    let e = wrap("the quick brown fox jumps over the lazy dog", 90.0, 10.0);
    assert!(e.len() >= 3);
    assert!(e.iter().all(|x| !x.starts_with(' ') && !x.ends_with(' ')));
    assert_eq!(e.join(" "), "the quick brown fox jumps over the lazy dog");
    // 比一行还长的词硬切
    let w = wrap("supercalifragilisticexpialidocious", 40.0, 10.0);
    assert!(w.len() >= 2 && w.concat() == "supercalifragilisticexpialidocious");
    // 换行符保留成段
    assert_eq!(wrap("a\nb", 100.0, 10.0), vec!["a", "b"]);
}

#[test]
fn fitting_shrinks_the_font_until_it_fits_but_not_below_the_minimum() {
    let text = "这是一段比较长的译文，需要放进一个不大的框里，所以字号要缩小一些才放得下。";
    let f = fit(text, 100.0, 30.0, 12.0, 6.0);
    assert!(f.size < 12.0 && f.size >= 6.0, "{f:?}");
    assert!(f.lines.len() as f32 * f.leading <= 30.0 * 1.08 + 0.5 || f.size <= 6.01, "{f:?}");
    assert_eq!(f.lines.concat(), text);
    let roomy = fit("短", 200.0, 100.0, 12.0, 6.0);
    assert_eq!(roomy.size, 12.0, "放得下就不缩");
    let tiny = fit(&"很长很长".repeat(40), 30.0, 10.0, 12.0, 6.0);
    assert!((tiny.size - 6.0).abs() < 0.01, "最小 6 磅：{tiny:?}");
}

/// 造一个只有两个表的最小 TTC（一个字体），看拆出来的 .ttf 目录和数据对不对。
#[test]
fn first_face_of_a_collection_becomes_a_standalone_font() {
    let t1 = b"HEADDATA12345".to_vec(); // 13 字节，要补齐到 16
    let t2 = b"CMAPDATA".to_vec();
    let mut f = vec![];
    f.extend_from_slice(b"ttcf");
    f.extend_from_slice(&[0, 1, 0, 0]); // 版本 1.0
    f.extend_from_slice(&1u32.to_be_bytes()); // 字体数
    let font_off = 16u32;
    f.extend_from_slice(&font_off.to_be_bytes());
    // 字体的偏移表：sfnt 版本 + 表数量 + 3 个范围字段
    f.extend_from_slice(&[0, 1, 0, 0]);
    f.extend_from_slice(&2u16.to_be_bytes());
    f.extend_from_slice(&[0, 32, 0, 1, 0, 0]);
    let dir_end = 16 + 12 + 2 * 16;
    let o1 = dir_end as u32;
    let o2 = o1 + 16;
    for (tag, sum, off, len) in [(b"head", 0xAAAA_0001u32, o1, t1.len() as u32), (b"cmap", 0xBBBB_0002, o2, t2.len() as u32)] {
        f.extend_from_slice(tag);
        f.extend_from_slice(&sum.to_be_bytes());
        f.extend_from_slice(&off.to_be_bytes());
        f.extend_from_slice(&len.to_be_bytes());
    }
    f.extend_from_slice(&t1);
    f.resize(o2 as usize, 0);
    f.extend_from_slice(&t2);
    let ttf = ttc_first_face(&f).unwrap();
    assert_eq!(&ttf[..4], &[0, 1, 0, 0]);
    assert_eq!(u16::from_be_bytes([ttf[4], ttf[5]]), 2);
    let rec = |i: usize| {
        let o = 12 + i * 16;
        (
            ttf[o..o + 4].to_vec(),
            u32::from_be_bytes([ttf[o + 4], ttf[o + 5], ttf[o + 6], ttf[o + 7]]),
            u32::from_be_bytes([ttf[o + 8], ttf[o + 9], ttf[o + 10], ttf[o + 11]]) as usize,
            u32::from_be_bytes([ttf[o + 12], ttf[o + 13], ttf[o + 14], ttf[o + 15]]) as usize,
        )
    };
    let (tag, sum, off, len) = rec(0);
    assert_eq!((tag.as_slice(), sum, len), (b"head".as_slice(), 0xAAAA_0001, 13));
    assert_eq!(&ttf[off..off + len], t1.as_slice());
    assert_eq!(off % 4, 0);
    let (tag, _, off, len) = rec(1);
    assert_eq!(tag.as_slice(), b"cmap");
    assert_eq!(&ttf[off..off + len], t2.as_slice());
    assert_eq!(off % 4, 0, "表数据 4 字节对齐");
    // 不是集合：原样返回；坏数据：报错而不是越界
    assert_eq!(ttc_first_face(&[0, 1, 0, 0, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]).unwrap().len(), 17);
    assert!(ttc_first_face(b"ttcf\0\x01\0\0\0\0\0\x01\0\0\xff\xff").is_err());
}

#[test]
fn background_is_the_median_of_the_pixels_around_the_box() {
    let (w, h) = (20usize, 20usize);
    let mut img = vec![0u8; w * h * 4];
    for p in img.chunks_exact_mut(4) {
        p.copy_from_slice(&[250, 240, 200, 255]); // 米色底
    }
    // 框里全是黑字，不会影响框外一圈
    for y in 8..12 {
        for x in 8..12 {
            let i = (y * w + x) * 4;
            img[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    assert_eq!(sample_bg(&img, w, h, (8, 8, 11, 11)), [250, 240, 200]);
    assert_eq!(sample_bg(&[], 0, 0, (0, 0, 1, 1)), [255, 255, 255]);
    assert_eq!(contrast([250, 240, 200]), [20, 20, 20]);
    assert_eq!(contrast([10, 20, 40]), [245, 245, 245]);
}

#[test]
fn component_names_match_the_official_release_files() {
    assert_eq!(asset_name("windows", "x86_64"), Some("pdfium-win-x64.tgz"));
    assert_eq!(asset_name("windows", "aarch64"), Some("pdfium-win-arm64.tgz"));
    assert_eq!(asset_name("macos", "aarch64"), Some("pdfium-mac-univ.tgz"));
    assert_eq!(asset_name("linux", "x86_64"), Some("pdfium-linux-x64.tgz"));
    assert_eq!(asset_name("freebsd", "x86_64"), None);
    assert_eq!(lib_file("windows"), "pdfium.dll");
    assert_eq!(lib_file("macos"), "libpdfium.dylib");
    assert_eq!(lib_file("linux"), "libpdfium.so");
    assert_eq!(Mode::parse("interleave"), Mode::Interleave);
    assert_eq!(Mode::parse("x"), Mode::InPlace);
}

#[test]
fn tgz_extraction_picks_only_the_library_by_name() {
    use flate2::{write::GzEncoder, Compression};
    let dir = std::env::temp_dir().join(format!("cc-pdfx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let arc = dir.join("a.tgz");
    {
        let gz = GzEncoder::new(std::fs::File::create(&arc).unwrap(), Compression::fast());
        let mut tb = tar::Builder::new(gz);
        for (name, body) in [("LICENSE", b"x".as_slice()), ("lib/libpdfium.so", b"ELFDATA".as_slice()), ("include/fpdfview.h", b"h".as_slice())] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tb.append_data(&mut h, name, body).unwrap();
        }
        tb.into_inner().unwrap().finish().unwrap();
    }
    let out = dir.join("out");
    assert!(untar_gz_pick(&arc, &out, "libpdfium.so").unwrap());
    assert_eq!(std::fs::read(out.join("libpdfium.so")).unwrap(), b"ELFDATA");
    assert!(!out.join("LICENSE").exists());
    assert!(!untar_gz_pick(&arc, &out, "pdfium.dll").unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 真正加载 PDFium 的集成测试：造一份 PDF → 读出块 → 写回译文 → 再读出来核对。
/// 需要环境变量 `CC_PDFIUM_LIB`（libpdfium 的路径）和 `CC_TEST_FONT`（任意 .ttf 字体）；平时不跑，CI 里下载组件后用 `--ignored` 跑。
#[test]
#[ignore = "needs the PDFium library; run with CC_PDFIUM_LIB and CC_TEST_FONT set"]
fn pdfium_roundtrip_reads_blocks_and_writes_the_translation_in_place() {
    use pdfium_render::prelude::*;
    let lib = PathBuf::from(std::env::var("CC_PDFIUM_LIB").expect("CC_PDFIUM_LIB"));
    let font = std::fs::read(std::env::var("CC_TEST_FONT").expect("CC_TEST_FONT")).unwrap();
    let dir = std::env::temp_dir().join(format!("cc-pdfx-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("in.pdf");
    {
        let pdfium = bind(&lib).unwrap();
        let mut doc = pdfium.create_new_pdf().unwrap();
        let helv = doc.fonts_mut().helvetica();
        let mut page = doc.pages_mut().create_page_at_start(PdfPagePaperSize::a4()).unwrap();
        for (i, (y, size, t)) in [
            (760.0, 16.0, "Attention Is All You Need"),
            (720.0, 10.0, "An attention function maps a query and key-value pairs"),
            (707.0, 10.0, "to an output, where all of them are vectors."),
        ]
        .into_iter()
        .enumerate()
        {
            let _ = i;
            page.objects_mut().create_text_object(PdfPoints::new(72.0), PdfPoints::new(y), t, helv, PdfPoints::new(size)).unwrap();
        }
        doc.save_to_file(&src).unwrap();
    }
    let pages = extract(&lib, &src).unwrap();
    assert_eq!(pages.len(), 1);
    assert!(pages[0].has_text);
    let texts: Vec<&str> = pages[0].blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Attention Is All You Need", "An attention function maps a query and key-value pairs to an output, where all of them are vectors."],
        "{texts:?}"
    );
    // 写回译文（用测试字体，字形可能缺，但文字对象要在原位置）
    let mut plan = PagePlan::default();
    for (b, tr) in pages[0].blocks.iter().zip(["Translated title", "Translated paragraph with several words that need wrapping inside the same box."]) {
        plan.items.push(Item { remove: b.runs.clone(), rect: (b.l, b.b, b.r.max(b.l + 300.0), b.t), size: b.size, rgb: b.rgb, bg: None, text: tr.into() });
    }
    let out = dir.join("out.pdf");
    apply(&lib, &font, &src, &out, &[plan.clone()], Mode::InPlace, |_, _| {}).unwrap();
    let after = extract(&lib, &out).unwrap();
    let all: String = after[0].blocks.iter().map(|b| b.text.clone()).collect::<Vec<_>>().join(" | ");
    assert!(all.contains("Translated title"), "{all}");
    assert!(!all.contains("Attention Is All You Need"), "原文要被删掉：{all}");
    assert!(after[0].blocks.iter().all(|b| b.t <= 800.0 && b.l >= 60.0), "译文在页面里");
    // 交替模式：页数翻倍
    let out2 = dir.join("out2.pdf");
    apply(&lib, &font, &src, &out2, &[plan], Mode::Interleave, |_, _| {}).unwrap();
    assert_eq!(extract(&lib, &out2).unwrap().len(), 2);
    // 渲染一页做扫描识字用
    let (w, h, rgba) = render_page(&lib, &src, 0, 400).unwrap();
    assert_eq!((w, rgba.len()), (400, w * h * 4));
    assert!(png_of_rgba(w, h, &rgba).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
    let _ = std::fs::remove_dir_all(&dir);
}
