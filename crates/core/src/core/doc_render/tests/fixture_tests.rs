//! CI 可运行的真实链路 fixture 测试（不依赖 Word/LibreOffice）。
//!
//! 由 `doc_render.rs` 的 `tests` 模块挂载（`mod fixture_tests;`）。本文件读取
//! `crates/core/tests/fixtures/` 下固定的 PDF + body XML：固定 PDF 由 pdfium
//! 直接栅格化，避免 E2E 的 Word/LibreOffice 依赖，使表格管线回归
//! （擦字/边框保留/区域锚定）在 CI 三平台都能跑到。
//!
//! # fixture 内容
//!
//! `table_fixture_document.xml` 是一张 2 列 × 3 行的表格（`tblBorders` 全
//! single 1pt 黑色，即 Table Grid 风格的实线边框），正文用 ASCII：
//!
//! | Item（居中） | Value（居中） |
//! |-------------|--------------|
//! | A1（左对齐） | 1（左对齐，单字格） |
//! | B2（左对齐） | {{A}}（左对齐，标签格，可见文本 A） |
//!
//! 第 2 行两个 run 显式带 `<w:sz w:val="21"/>`（10.5pt），使
//! `TableCellView.sz_half_pt = Some(21)`：数字 `1` 是 ASCII 单字格，不参与
//! 字符流匹配、也拿不到全角字形校准字号，字号只能走 `sz_half_pt` 回退
//! （10.5pt × 200 / 72 ≈ 29px），正好锁住这条回退链。
//!
//! 列宽特意取 3000 twips（150pt）而不是整页宽度：表头居中、正文左对齐时，
//! 单字格区域要靠「行带内反查真实字形」兜底，而该兜底有「候选字形中心距推断
//! 矩形中心 ≤ max(4×格宽, 200px)」的距离闸门；列太宽（如 2768 twips 的 3 列
//! 表在 200dpi 下约 553px）会让左对齐单字与居中表头相距 >200px，闸门拒绝
//! 后区域退回列中心。150pt 列宽下该距离约 185px，闸门可通过。
//!
//! 行高显式取 440 twips（22pt，`hRule="atLeast"`）：单字格区域最小高度是
//! `fs+6=35px`，若行高按默认单行行高（约 12pt≈39px）排版，35px 高的区域
//! 下缘会压到行底边框，使「区域内部无暗像素」断言把边框像素误算成印刷字
//! 残留；22pt 行高下区域上缘距行顶边框约 5px、下缘距行底边框约 24px
//!（实测值）。即便如此，测试也只统计字形墨迹带，避免 1px 级边框漂移。
//!
//! # fixture 生成方法（一次性，测试运行时不执行）
//!
//! 1. 以本目录 `table_fixture_document.xml` 作为 `word/document.xml`，与三个
//!    固定部件一起打成最小 docx：
//!    - `[Content_Types].xml`（Default rels/xml + document.xml Override）
//!    - `_rels/.rels`（officeDocument → word/document.xml）
//!    - `word/_rels/document.xml.rels`（空 Relationships）
//! 2. 用 LibreOffice 26.8.1.1 headless 转 PDF（Word COM 输出亦可，但需重新
//!    核对所有实测值；本 fixture 用 LibreOffice 生成）：
//!
//!    ```text
//!    & "C:\Program Files\LibreOffice\program\soffice.com" `
//!        "-env:UserInstallation=file:///<临时目录>/lo_profile" `
//!        --headless --norestore --convert-to pdf --outdir <临时目录> table_fixture.docx
//!    ```
//!
//!    注意用 `soffice.com`（控制台包装器）而非 `soffice.exe`：后者在部分
//!    Windows 环境下不等待转换完成、也不回传退出码。
//! 3. 产物改名为 `table_fixture.pdf` 放回本目录；`word/document.xml` 原样存为
//!    `table_fixture_document.xml`。生成用的 docx 属一次性中间产物，不提交。
//!
//! 测试只读取这两个静态文件，**不调用 Word / LibreOffice / soffice**，因此
//! CI 三平台可直接运行；pdfium 动态库由 build.rs 内嵌（Windows 取仓库根
//! `pdfium.dll`，Linux/macOS 取 `crates/core/assets/`），测试不切换当前目录。

use super::*;

use std::path::PathBuf;

use crate::core::docx_io::{parse_body_stream_for_test, StyleRegistry};

/// 由 LibreOffice headless 一次性生成的固定 PDF（见模块注释）。
const FIXTURE_PDF: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/table_fixture.pdf"));
/// 与 PDF 同一份 docx 的 `word/document.xml`（body 流解析输入）。
const FIXTURE_DOCUMENT_XML: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/table_fixture_document.xml"
));

const DPI: u32 = 200;
/// 暗像素阈值：与生产擦字判据无关，仅用于 fixture 断言（RGB 均值 luma）。
const DARK_LUMA: u32 = 160;

/// 每次运行独立的临时输出目录（不删除：便于人工查看底图 PNG）。
fn unique_out_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("handwrite_sim_fixture_{}_{stamp}", std::process::id()))
}

fn luma(p: &image::Rgb<u8>) -> u32 {
    (p.0[0] as u32 + p.0[1] as u32 + p.0[2] as u32) / 3
}

fn is_dark(p: &image::Rgb<u8>) -> bool {
    luma(p) < DARK_LUMA
}

/// [x0,x1) × [y0,y1) 内暗像素计数（越界自动裁剪）。
fn dark_pixels(img: &image::RgbImage, x0: i32, y0: i32, x1: i32, y1: i32) -> u32 {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let x0 = x0.clamp(0, w);
    let x1 = x1.clamp(0, w);
    let y0 = y0.clamp(0, h);
    let y1 = y1.clamp(0, h);
    if x1 <= x0 || y1 <= y0 {
        return 0;
    }
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if is_dark(img.get_pixel(x as u32, y as u32)) {
                n += 1;
            }
        }
    }
    n
}

/// [x0,x1) × [y0,y1) 内 luma 落在 `[lo, hi)` 的像素计数（越界自动裁剪）。
/// 用于把“深色印刷字残留”（lo=0/hi=DARK_LUMA）与“浅灰抗锯齿边”分开统计。
fn pixels_in_luma_band(
    img: &image::RgbImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    lo: u32,
    hi: u32,
) -> u32 {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let x0 = x0.clamp(0, w);
    let x1 = x1.clamp(0, w);
    let y0 = y0.clamp(0, h);
    let y1 = y1.clamp(0, h);
    if x1 <= x0 || y1 <= y0 {
        return 0;
    }
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let l = luma(img.get_pixel(x as u32, y as u32));
            if l >= lo && l < hi {
                n += 1;
            }
        }
    }
    n
}

/// 一行内最长的连续暗像素 run 长度。
fn longest_dark_run_in_row(img: &image::RgbImage, y: i32, x0: i32, x1: i32) -> i32 {
    let mut best = 0;
    let mut cur = 0;
    for x in x0..x1 {
        if is_dark(img.get_pixel(x as u32, y as u32)) {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// 一列内最长的连续暗像素 run 长度。
fn longest_dark_run_in_col(img: &image::RgbImage, x: i32, y0: i32, y1: i32) -> i32 {
    let mut best = 0;
    let mut cur = 0;
    for y in y0..y1 {
        if is_dark(img.get_pixel(x as u32, y as u32)) {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// 一条（可能占多像素厚的）直线。
#[derive(Debug, Clone, Copy)]
struct LineGroup {
    first: i32,
    last: i32,
    center: i32,
    thickness: i32,
    longest_run: i32,
}

/// 整页直线扫描结果：`max_run` 为全页最长 run（≈ 表宽/表高），
/// `groups` 为按相邻坐标归并后的长直线。
struct LineScan {
    max_run: i32,
    groups: Vec<LineGroup>,
}

impl LineScan {
    /// `profile` 为 (坐标, 该坐标上的最长暗 run)。判定为直线的条件：
    /// run ≥ `min_run`（绝对下限，滤掉文字笔画）且 run ≥ max_run 的 50%
    ///（相对下限，对应“长横线 ≥ 表宽 50%”），相邻坐标 gap ≤ 2 归并为同一条线。
    fn from_profile(profile: &[(i32, i32)], min_run: i32) -> Self {
        let max_run = profile.iter().map(|&(_, r)| r).max().unwrap_or(0);
        let mut groups: Vec<LineGroup> = Vec::new();
        for &(idx, run) in profile {
            if run < min_run || run * 2 < max_run {
                continue;
            }
            if let Some(last) = groups.last_mut() {
                if idx - last.last <= 2 {
                    last.last = idx;
                    last.longest_run = last.longest_run.max(run);
                    continue;
                }
            }
            groups.push(LineGroup {
                first: idx,
                last: idx,
                center: idx,
                thickness: 1,
                longest_run: run,
            });
        }
        for g in &mut groups {
            g.center = (g.first + g.last) / 2;
            g.thickness = g.last - g.first + 1;
        }
        Self { max_run, groups }
    }

    fn centers(&self) -> Vec<i32> {
        self.groups.iter().map(|g| g.center).collect()
    }
}

/// 找 `at` 左右两侧最近的直线（用于按区域中心反查所在单元格的边框）。
fn bracket(centers: &[i32], at: i32) -> Option<(i32, i32)> {
    let left = centers.iter().copied().filter(|&c| c < at).max()?;
    let right = centers.iter().copied().filter(|&c| c > at).min()?;
    Some((left, right))
}

#[test]
fn pdf_table_fixture_end_to_end() {
    let out_dir = unique_out_dir();
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("创建临时输出目录失败");
    let pdf_path = out_dir.join("table_fixture.pdf");
    std::fs::write(&pdf_path, FIXTURE_PDF).expect("写出 fixture PDF 失败");

    // ---- body 流：从固定 document.xml 解析出表格结构 ----
    let xml = std::str::from_utf8(FIXTURE_DOCUMENT_XML).expect("fixture document.xml 不是 UTF-8");
    let mut registry = StyleRegistry::new();
    let stream =
        parse_body_stream_for_test(xml, &mut registry).expect("解析 fixture document.xml 失败");
    let table_count = stream
        .iter()
        .filter(|e| matches!(e, crate::core::docx_io::BodyElement::Table(_)))
        .count();
    assert_eq!(table_count, 1, "fixture body 流应恰好 1 张表");

    // ---- 真实链路：固定 PDF → pdfium 栅格化 → 表格定位/擦字 → 单元格区域 ----
    // 不 set_current_dir：open_pdfium 会走 build.rs 内嵌库自解压，避免干扰并行测试。
    let (paths, regions) = pdf_to_images_opt_with_tables(&pdf_path, &out_dir, DPI, true, &stream)
        .expect("PDF fixture 真实链路执行失败（pdfium 加载 / 栅格化 / 表格管线）");

    println!("输出目录 = {}", out_dir.display());
    println!("页数 = {}，区域总数 = {}", paths.len(), regions.len());
    for r in &regions {
        println!(
            "  region: text={:?} table={:?} fs={} rect=({},{},{}x{}) align={} printed={} highlight={:?}",
            r.text, r.table_index, r.font_size, r.x, r.y, r.w, r.h, r.align, r.printed, r.highlight
        );
    }

    assert_eq!(paths.len(), 1, "fixture 是单页文档，应只栅格化出 1 页");
    assert!(paths[0].is_file(), "底图 PNG 未生成：{}", paths[0].display());

    let img = image::open(&paths[0]).expect("读取底图 PNG 失败").to_rgb8();
    let (pw, ph) = (img.width() as i32, img.height() as i32);
    println!("底图 = {} （{pw}x{ph}）", paths[0].display());

    // ---- 表格区域 ----
    let table_regions: Vec<&TextRegion> =
        regions.iter().filter(|r| r.table_index == Some(1)).collect();
    let texts: Vec<&str> = table_regions.iter().map(|r| r.text.as_str()).collect();
    println!("表格区域数 = {}，文本 = {:?}", table_regions.len(), texts);

    for expect in ["Item", "Value", "A1", "1", "B2", "A"] {
        assert!(
            texts.contains(&expect),
            "缺少单元格区域 {expect:?}（标签 {{A}} 的可见文本应为 A），实际 {texts:?}"
        );
    }
    assert!(
        regions.iter().all(|r| r.page == 1),
        "单页文档的所有区域 page 都应为 1：{:?}",
        regions.iter().map(|r| (r.text.as_str(), r.page)).collect::<Vec<_>>()
    );
    // 表内标签必须由单元格接管：不能冒出独立的 table_index == None 区域。
    // 单字标签格（如 {{A}}）不参与字符流匹配，其推断矩形来自同列居中表头的
    // 列带，与标签框不重叠——生产侧已把「锚定后的最终区域矩形」追加进
    // cell_rects 供标签排重，这里锁定该回归。
    let outside: Vec<&TextRegion> = regions.iter().filter(|r| r.table_index.is_none()).collect();
    assert!(outside.is_empty(), "表内标签不应生成表外区域：{outside:?}");

    // 表头居中、正文左对齐（w:jc 语义，跨平台稳定）：顺带验证对齐透传
    let item = table_regions.iter().find(|r| r.text == "Item").expect("缺少 Item 区域");
    let value = table_regions.iter().find(|r| r.text == "Value").expect("缺少 Value 区域");
    let a1 = table_regions.iter().find(|r| r.text == "A1").expect("缺少 A1 区域");
    let b2 = table_regions.iter().find(|r| r.text == "B2").expect("缺少 B2 区域");
    assert_eq!(item.align, 1, "表头 Item 居中");
    assert_eq!(value.align, 1, "表头 Value 居中");
    assert_eq!(a1.align, 0, "A1 左对齐");
    assert_eq!(b2.align, 0, "B2 左对齐");

    // ---- 单字格 `1`：字号回退 + 最小尺寸 + 字形锚定 ----
    let one = table_regions
        .iter()
        .find(|r| r.text == "1")
        .expect("单字格 1 未生成区域（整行未定位或推断失败）");
    println!(
        "单字格 1: fs={} rect=({},{},{}x{}) | fs+10={} fs+6={}",
        one.font_size,
        one.x,
        one.y,
        one.w,
        one.h,
        one.font_size + 10,
        one.font_size + 6
    );
    // 10.5pt（w:sz=21）× 200dpi / 72 ≈ 29px：ASCII 单字格拿不到全角字形校准，
    // 必须走 sz_half_pt 回退；若该回退断链 font_size 会是 0，下面的尺寸断言会
    // 退化成恒真，所以这里先锁死字号。
    assert_eq!(one.font_size, 29, "单字格字号应回退到 10.5pt×200/72≈29px");
    assert!(
        one.w >= one.font_size + 10,
        "单字格区域宽度不足（手写字会被裁切）：w={} < fs+10={}",
        one.w,
        one.font_size + 10
    );
    assert!(
        one.h >= one.font_size + 6,
        "单字格区域高度不足（手写字会被裁切）：h={} < fs+6={}",
        one.h,
        one.font_size + 6
    );

    // ---- 页面像素：表格线检测（擦字不得把边框擦断）----
    let x_range = (0, pw);
    let y_range = (0, ph);
    let h_profile: Vec<(i32, i32)> = (0..ph)
        .map(|y| (y, longest_dark_run_in_row(&img, y, x_range.0, x_range.1)))
        .collect();
    let v_profile: Vec<(i32, i32)> = (0..pw)
        .map(|x| (x, longest_dark_run_in_col(&img, x, y_range.0, y_range.1)))
        .collect();
    let h_scan = LineScan::from_profile(&h_profile, 150);
    let v_scan = LineScan::from_profile(&v_profile, 60);
    println!(
        "横线：max_run={} 条数={} {:?}",
        h_scan.max_run,
        h_scan.groups.len(),
        h_scan.groups
    );
    println!(
        "竖线：max_run={} 条数={} {:?}",
        v_scan.max_run,
        v_scan.groups.len(),
        v_scan.groups
    );
    // 2 列 3 行的实线表：4 条横线（外框 2 + 行分隔 2）、3 条竖线（外框 2 + 列分隔 1）。
    // 条数按 fixture 几何精确断言（固定 PDF + pdfium 栅格化，几何不随平台变），
    // 否则「某条边框整条消失」会因条数下限太低被放过；每条线的最长 run 另由
    // ≥ 全页最长 85% 的断言保证不被擦出缺口。
    assert!(
        h_scan.groups.len() >= 4,
        "横向表格线应 4 条（实测 {}）",
        h_scan.groups.len()
    );
    assert!(
        v_scan.groups.len() >= 3,
        "竖向表格线应 3 条（实测 {}）",
        v_scan.groups.len()
    );
    for g in &h_scan.groups {
        assert!(
            g.longest_run * 100 >= h_scan.max_run * 85,
            "横线被擦断：run {} 低于全页最长 {} 的 85%（{g:?}）",
            g.longest_run,
            h_scan.max_run
        );
    }
    for g in &v_scan.groups {
        assert!(
            g.longest_run * 100 >= v_scan.max_run * 85,
            "竖线被擦断：run {} 低于全页最长 {} 的 85%（{g:?}）",
            g.longest_run,
            v_scan.max_run
        );
    }
    let v_centers = v_scan.centers();
    let h_centers = h_scan.centers();

    // ---- 单字格 `1` 的印刷字被擦除 ----
    // 区域高度现在恰好 = fs+6（最小尺寸下限），区域下缘距行底边框可能只有几像素，
    // 不同平台 pdfium 的抗锯齿/取整会让边框像素贴着区域边缘；若整块计数，边框会被
    // 误判成“印刷字残留”（本机首版紧行高实测 35 个暗像素全部来自行底边框）。
    // 因此只在「字形墨迹带」内计数：整体向内收 2px，同时把上下界钳到所在行边框
    // 内侧 2px。边框自身的完好性由上面的长直线 run 断言单独保证。
    let (one_row_top, one_row_bottom) =
        bracket(&h_centers, one.y + one.h / 2).expect("单字格 1 外侧应能定位上下横线");
    let one_dark_x0 = one.x + 2;
    let one_dark_x1 = one.x + one.w - 2;
    let one_dark_y0 = (one.y + 2).max(one_row_top + 2);
    let one_dark_y1 = (one.y + one.h - 2).min(one_row_bottom - 2);
    assert!(
        one_dark_x1 > one_dark_x0 && one_dark_y1 > one_dark_y0,
        "单字格 1 的计数带为空：x {one_dark_x0}..{one_dark_x1}, y {one_dark_y0}..{one_dark_y1}"
    );
    let one_dark = dark_pixels(&img, one_dark_x0, one_dark_y0, one_dark_x1, one_dark_y1);
    let one_gray = pixels_in_luma_band(
        &img, one_dark_x0, one_dark_y0, one_dark_x1, one_dark_y1, DARK_LUMA, 235,
    );
    println!(
        "单字格 1 区域内部暗像素（内收 2px，字形墨迹带 y={one_dark_y0}..{one_dark_y1}，行边框 y={one_row_top}/{one_row_bottom}）= {one_dark}；浅灰抗锯齿像素（luma {DARK_LUMA}..235）= {one_gray}"
    );
    assert!(
        one_dark <= 5,
        "单字格 1 的印刷字未被擦净：字形墨迹带内暗像素 {one_dark} > 5"
    );

    // ---- 锚定：单字格区域应贴该列左侧文字起点，而不是列中心 ----
    assert!(v_centers.len() >= 3, "需要 3 条竖线定位左右列，实测 {v_centers:?}");
    let (v_left, v_mid) = (v_centers[0], v_centers[1]);
    let v_right = v_centers[2];
    // 锚定口径（fixture 几何前提）：两列同为 3000 twips（等宽），A1 与 1 都
    // 左对齐且单元格左边距相同，故两者字形左缘恰好相差一个列宽（左右竖线
    // 中心距）；命中字形后区域左缘都取「字形左缘 - 8」。跨平台只有 pdfium
    // 抗锯齿/取整带来 ≤1px 漂移，文档布局几何不变，因此用 ±20px 容差、
    // 不做精确坐标断言。
    let expected_one_x = a1.x + (v_mid - v_left);
    println!(
        "锚定：a1.x={} 列宽={} 期望 one.x≈{} 实测 one.x={} 竖线=({v_left},{v_mid},{v_right})",
        a1.x,
        v_mid - v_left,
        expected_one_x,
        one.x
    );
    assert!(
        (one.x - expected_one_x).abs() <= 20,
        "单字格 1 未锚定到列左侧文字起点：one.x={}，期望 {expected_one_x}±20",
        one.x
    );
    let col2_center = (v_mid + v_right) / 2;
    assert!(
        one.x + one.w / 2 < col2_center - 100,
        "单字格 1 落在列中心而非左侧文字起点：区域中心 {} vs 列中心 {col2_center}",
        one.x + one.w / 2
    );
    assert!(
        one.x > v_mid + 2,
        "单字格 1 区域左缘压到列边框（擦字会吃线）：one.x={} 边框={v_mid}",
        one.x
    );

    // ---- 标签 `{{A}}` 被擦除：所在单元格内部无深色残留 ----
    // 标签字形由 extract_pdf_page_tags 逐字形擦白（即使落在单元格内、不生成独立区域）；
    // 单元格路径只擦命中正文（这里是 A），`{{`/`}}` 由标签路径擦掉。
    // 用 A 区域中心反查所在单元格的边框，检查整个单元格内部（内收 4px）无深色残留。
    let a_cell = table_regions.iter().find(|r| r.text == "A").expect("缺少标签格 A 区域");
    let (tag_v0, tag_v1) = bracket(&v_centers, a_cell.x + a_cell.w / 2)
        .expect("标签格 A 外侧应能定位左右竖线");
    let (tag_h0, tag_h1) = bracket(&h_centers, a_cell.y + a_cell.h / 2)
        .expect("标签格 A 外侧应能定位上下横线");
    let tag_dark = dark_pixels(&img, tag_v0 + 4, tag_h0 + 4, tag_v1 - 4, tag_h1 - 4);
    let tag_gray = pixels_in_luma_band(
        &img,
        tag_v0 + 4,
        tag_h0 + 4,
        tag_v1 - 4,
        tag_h1 - 4,
        DARK_LUMA,
        235,
    );
    println!(
        "标签格 A 单元格内部暗像素 = {tag_dark}，浅灰抗锯齿像素（luma {DARK_LUMA}..235）= {tag_gray}（单元格 x {tag_v0}..{tag_v1}, y {tag_h0}..{tag_h1}）"
    );
    assert!(
        tag_dark <= 5,
        "{{{{A}}}} 标签未擦净：标签格内部暗像素 {tag_dark} > 5（{{{{ }}}} 与 A 都应被逐字形擦除）"
    );

    // ---- 同一单元格里的 `1` 也应被擦净（交叉验证：与区域矩形无关的独立测量）----
    let (one_v0, one_v1) = bracket(&v_centers, one.x + one.w / 2)
        .expect("单字格 1 外侧应能定位左右竖线");
    let (one_h0, one_h1) = bracket(&h_centers, one.y + one.h / 2)
        .expect("单字格 1 外侧应能定位上下横线");
    let one_cell_dark = dark_pixels(&img, one_v0 + 4, one_h0 + 4, one_v1 - 4, one_h1 - 4);
    let one_cell_gray = pixels_in_luma_band(
        &img,
        one_v0 + 4,
        one_h0 + 4,
        one_v1 - 4,
        one_h1 - 4,
        DARK_LUMA,
        235,
    );
    println!(
        "单字格 1 单元格内部暗像素 = {one_cell_dark}，浅灰抗锯齿像素（luma {DARK_LUMA}..235）= {one_cell_gray}（单元格 x {one_v0}..{one_v1}, y {one_h0}..{one_h1}）"
    );
    assert!(
        one_cell_dark <= 5,
        "单字格 1 所在单元格仍有印刷字残留：{one_cell_dark} > 5"
    );

    println!("全部断言通过；底图保留在 {}", paths[0].display());
}
