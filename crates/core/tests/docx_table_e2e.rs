//! docx 表格识别导入·端到端手工验证测试（Task 9）。
//!
//! 链路：真实 docx 文件 → Word COM（或 LibreOffice）转 PDF → pdfium 栅格化
//! → 表格定位/擦字 → 单元格 TextRegion 生成 → 表格线保留校验。
//!
//! 依赖本机环境（Word/LibreOffice + 仓库根目录 pdfium.dll），默认 `#[ignore]`，
//! 手工运行：
//! ```bash
//! cargo test -p handwrite-sim --test docx_table_e2e -- --ignored --nocapture
//! ```
//! 输出目录固定为系统临时目录 `handwrite_sim_e2e_table`，便于人工查看底图 PNG。

use std::io::Write;
use std::path::Path;

use handwrite_sim::core::doc_render::document_to_page_images_opt;
use handwrite_sim::core::models::TextRegion;

/// 端到端验证整条导入链路。四个验证点：
/// 1. 表格区域数 = 7（3×3 表：9 格 − 合并续行 1 − 空格 1）；
/// 2. 黄色高亮格 highlight == Some("yellow") 且 role_id == 2；
/// 3. 底图 PNG 中表格边框线仍为深色（长暗直线检测）；
/// 4. `{{打印:xxx}}` 格生成 printed == true 的区域。
#[test]
#[ignore = "需要本机 Word（COM）或 LibreOffice，以及仓库根目录 pdfium.dll"]
fn docx_table_import_end_to_end() {
    // open_pdfium 依次探测 exe 同级与当前工作目录，切到仓库根以命中根目录 pdfium.dll
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("定位仓库根失败");
    std::env::set_current_dir(repo_root).expect("切换工作目录到仓库根失败");

    let out_dir = std::env::temp_dir().join("handwrite_sim_e2e_table");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("创建输出目录失败");
    let docx_path = out_dir.join("表格测试.docx");
    write_test_docx(&docx_path);

    let (paths, regions) =
        document_to_page_images_opt(&docx_path, &out_dir, 200, true).expect("整链路执行失败");

    println!("页数 = {}", paths.len());
    println!("总区域数 = {}", regions.len());
    for r in &regions {
        println!(
            "  region: text={:?} table={:?} highlight={:?} role={} printed={} align={} fs={} rect=({},{},{},{})",
            r.text, r.table_index, r.highlight, r.role_id, r.printed, r.align, r.font_size, r.x, r.y, r.w, r.h
        );
    }
    assert_eq!(paths.len(), 1, "应为单页文档");

    // ---- 验证点 1：表格单元格区域数 ----
    // 第三行同时锁定两条短格规则："乙项"（2 字）正常定位出行带，
    // "丙"（1 字）不参与匹配、由行带 × 列带推算矩形（带推断兜底）。
    let table_regions: Vec<&TextRegion> =
        regions.iter().filter(|r| r.table_index == Some(1)).collect();
    let texts: Vec<&str> = table_regions.iter().map(|r| r.text.as_str()).collect();
    println!("表格区域数 = {}，文本 = {:?}", table_regions.len(), texts);
    assert_eq!(
        table_regions.len(),
        7,
        "3×3 表（1 个纵向合并 + 1 个空格）：9 − 续行 1 − 空格 1 = 7"
    );
    for expect in ["项目", "数值", "备注", "合并说明", "手写答案", "乙项", "丙"] {
        assert!(
            texts.contains(&expect),
            "缺少单元格区域 {expect:?}，实际 {texts:?}"
        );
    }
    // {{打印:}} 标签已被单元格区域接管：不应再产生独立的表外标签区域
    let non_table: Vec<&TextRegion> = regions.iter().filter(|r| r.table_index.is_none()).collect();
    assert!(
        non_table.is_empty(),
        "表格文档不应有表外区域（标签应被单元格接管）：{:#?}",
        non_table
    );

    // ---- 验证点 2：黄色高亮格角色映射 ----
    let yellow = regions
        .iter()
        .find(|r| r.highlight.as_deref() == Some("yellow"))
        .expect("应存在 yellow 高亮区域");
    assert_eq!(yellow.text, "手写答案");
    assert_eq!(yellow.role_id, 2, "首个高亮色应映射到角色 2");
    assert_eq!(yellow.table_index, Some(1));

    // ---- 验证点 4：{{打印:xxx}} 格生成印刷体区域 ----
    let printed_regions: Vec<&TextRegion> = regions.iter().filter(|r| r.printed).collect();
    println!("printed 区域数 = {}", printed_regions.len());
    assert_eq!(printed_regions.len(), 1, "仅 {{打印:备注}} 一格为印刷体");
    assert_eq!(printed_regions[0].text, "备注");
    assert_eq!(printed_regions[0].table_index, Some(1));

    // ---- 验证点 3：表格边框线保留（底图长暗直线检测）----
    let png_path = &paths[0];
    println!("底图 = {}", png_path.display());
    let img = image::open(png_path).expect("读取底图 PNG 失败").to_rgb8();
    let (w, h) = (img.width() as i32, img.height() as i32);
    println!("底图尺寸 = {w}x{h}");

    // 表格外接框 = 表格区域并集外扩 40px（区域矩形贴文字，边框线在其外侧）
    let mut bx0 = i32::MAX;
    let mut by0 = i32::MAX;
    let mut bx1 = i32::MIN;
    let mut by1 = i32::MIN;
    for r in &table_regions {
        bx0 = bx0.min(r.x);
        by0 = by0.min(r.y);
        bx1 = bx1.max(r.x + r.w as i32);
        by1 = by1.max(r.y + r.h as i32);
    }
    let m = 40i32;
    let x0 = (bx0 - m).max(0) as u32;
    let y0 = (by0 - m).max(0) as u32;
    let x1 = (bx1 + m).min(w) as u32;
    let y1 = (by1 + m).min(h) as u32;
    let lum = |p: image::Rgb<u8>| -> u32 {
        let [r, g, b] = p.0;
        (r as u32 + g as u32 + b as u32) / 3
    };

    // 水平线：某行中连续暗像素的最长 run ≥ 框宽 50%（表格线是页面上唯一
    // 的通栏暗元素，正文文字有字间隙，不会产生长连续 run）
    let box_w = x1 - x0;
    let need = (box_w as f32 * 0.5) as u32;
    let mut h_rows: Vec<(u32, u32, u32)> = Vec::new(); // (y, 最长 run 起点, 终点)
    for y in y0..y1 {
        let mut best = (0u32, 0u32, 0u32);
        let mut cur_start = 0u32;
        let mut cur = 0u32;
        for x in x0..x1 {
            if lum(*img.get_pixel(x, y)) < 160 {
                if cur == 0 {
                    cur_start = x;
                }
                cur += 1;
                if cur > best.0 {
                    best = (cur, cur_start, x + 1);
                }
            } else {
                cur = 0;
            }
        }
        if best.0 >= need {
            h_rows.push((y, best.1, best.2));
        }
    }
    // 相邻行聚类为线（线宽 1-4px）
    let clusters = |mut xs: Vec<u32>| -> Vec<(u32, u32)> {
        xs.sort_unstable();
        let mut out: Vec<(u32, u32)> = Vec::new();
        for v in xs {
            match out.last_mut() {
                Some(last) if v <= last.1 + 4 => last.1 = last.1.max(v),
                _ => out.push((v, v)),
            }
        }
        out
    };
    let h_lines = clusters(h_rows.iter().map(|&(y, _, _)| y).collect());
    println!(
        "水平线 {} 条：{:?}（3×3 表期望 ≥ 3：上边/表头分隔/下边；合并列处 1-2 行分隔为部分横线）",
        h_lines.len(),
        h_lines
    );
    assert!(h_lines.len() >= 3, "表格水平边框线应 ≥ 3 条，实际 {:?}", h_lines);

    // 竖直线：Word 输出的 0.5pt 竖线经抗锯齿会在相邻两列间漂移（单列 run 断裂），
    // 故合并相邻两列取"或"，且阈值放宽到 170。x 范围取水平线 run 的跨度（覆盖
    // 含右边框在内的完整表宽——单元格文字居中时区域并集达不到右表框）。
    let y_top = h_lines.first().unwrap().0;
    let y_bot = h_lines.last().unwrap().1;
    let span_y = y_bot - y_top;
    let need_v = (span_y as f32 * 0.6) as u32;
    let hx0 = h_rows.iter().map(|&(_, s, _)| s).min().unwrap();
    let hx1 = h_rows.iter().map(|&(_, _, e)| e).max().unwrap();
    let mut v_cols: Vec<u32> = Vec::new();
    for x in hx0..hx1 {
        let mut run = 0u32;
        let mut cur = 0u32;
        for y in y_top..=y_bot {
            let dark = lum(*img.get_pixel(x, y)) < 170
                || lum(*img.get_pixel((x + 1).min(w as u32 - 1), y)) < 170;
            if dark {
                cur += 1;
                run = run.max(cur);
            } else {
                cur = 0;
            }
        }
        if run >= need_v {
            v_cols.push(x);
        }
    }
    let v_lines = clusters(v_cols);
    println!("竖直线 {} 条：{:?}（3×3 表期望 ≥ 3）", v_lines.len(), v_lines);
    assert!(v_lines.len() >= 3, "表格竖直边框线应 ≥ 3 条，实际 {:?}", v_lines);
    println!("端到端验证全部通过；底图留存于 {}", out_dir.display());
}

/// 构造测试 docx：表格前正文 + 3×3 表（表头居中、纵向合并、黄色高亮、
/// `{{打印:备注}}` 印刷格、一个空格）。zip 各 part 与单元测试构造的最小包一致。
fn write_test_docx(path: &Path) {
    let document_xml = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:body>"#,
        r#"<w:p><w:r><w:t>表格前正文</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>"#,
        r#"<w:tblBorders>"#,
        r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"<w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"<w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"<w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
        r#"</w:tblBorders><w:tblLayout w:type="fixed"/>"#,
        // 单元格左右边距 108 twips（真实 Word 表格默认，如 Table Grid 样式）：
        // 无样式的裸表文字会贴着边框渲染，字符擦除的 2px 外扩会误伤表格线
        r#"<w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol w:w="2768"/><w:gridCol w:w="2768"/><w:gridCol w:w="2768"/></w:tblGrid>"#,
        // 表头行（居中）；第三格为 {{打印:}} 印刷格
        r#"<w:tr>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>项目</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>数值</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>{{打印:备注}}</w:t></w:r></w:p></w:tc>"#,
        r#"</w:tr>"#,
        // 第二行：纵向合并起始格（跨 2-3 行）、黄色高亮格、空格
        r#"<w:tr>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>合并说明</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>手写答案</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p/></w:tc>"#,
        r#"</w:tr>"#,
        // 第三行：合并续行格（无内容）、多字格 + 单字格
        // （"乙项" 2 字定位出行带，"丙" 1 字不匹配、走带推断——各覆盖一条规则）
        r#"<w:tr>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/><w:vMerge/></w:tcPr><w:p/></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>乙项</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="2768" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>丙</w:t></w:r></w:p></w:tc>"#,
        r#"</w:tr>"#,
        r#"</w:tbl>"#,
        r#"<w:p/>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr>"#,
        r#"</w:body></w:document>"#
    );
    let content_types: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let root_rels: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

    let file = std::fs::File::create(path).expect("创建 docx 失败");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", root_rels),
        ("word/document.xml", document_xml.as_bytes()),
        ("word/_rels/document.xml.rels", doc_rels),
    ] {
        zip.start_file(name, options).expect("写入 zip 条目失败");
        zip.write_all(data).expect("写入 zip 数据失败");
    }
    zip.finish().expect("完成 docx 打包失败");
}
