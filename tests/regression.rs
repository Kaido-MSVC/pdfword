//! 實際文件的回歸測試（需要 vendor/pdfium/pdfium.dll，先執行 scripts/fetch-pdfium.ps1）

use pdfword::model::{Block, Paragraph, PageLayout};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn layout(fixture: &str) -> Option<PageLayout> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dll = root.join("vendor/pdfium");
    if !dll.join("pdfium.dll").exists() {
        eprintln!("略過：找不到 pdfium.dll");
        return None;
    }
    pdfword::extract::init(&dll).unwrap();
    let doc = pdfword::extract::extract_document(&root.join("tests/fixtures").join(fixture), &AtomicBool::new(false), |_, _| {})
        .unwrap();
    let body = pdfword::layout::body_size(&doc.pages);
    Some(pdfword::layout::analyze(&doc.pages[0], body))
}

fn paragraphs(l: &PageLayout) -> Vec<&Paragraph> {
    l.blocks.iter().filter_map(|b| if let Block::Paragraph(p) = b { Some(p) } else { None }).collect()
}

/// 去掉空白、Tab 與填空用的全形空白
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 報關公司(行)承受切結書：DFKaiShu 字框異常、模擬粗體、以線條畫的填空底線
#[test]
fn customs_affidavit_form() {
    let Some(l) = layout("customs-affidavit.pdf") else { return };
    let ps = paragraphs(&l);

    // 標題是粗體
    let title = ps[0];
    assert_eq!(compact(&title.text()), "財政部關務署基隆關報關公司(行)承受切結書");
    assert!(title.runs.iter().filter(|r| !r.tab).all(|r| r.bold), "標題應為粗體：{:?}", title.runs);

    // 「立切結書人為＿＿＿報關公司」整行順序正確，中間是加底線的空白
    let first = ps
        .iter()
        .find(|p| p.text().starts_with("立切結書人為"))
        .unwrap_or_else(|| panic!("找不到「立切結書人為」行：{:?}", ps.iter().map(|p| p.text()).collect::<Vec<_>>()));
    assert_eq!(compact(&first.text()), "立切結書人為報關公司");
    let shape: Vec<(bool, bool)> = first
        .runs
        .iter()
        .map(|r| (r.underline, r.text.chars().all(|c| c == '\u{3000}')))
        .collect();
    assert_eq!(shape, vec![(false, false), (true, true), (false, false)], "{:?}", first.runs);
    assert!(first.runs[1].text.chars().count() >= 8, "填空欄太短：{:?}", first.runs[1].text);
    assert!(!matches!(first.align, pdfword::model::Align::Center));

    // 「原負責人＿＿＿」與日期行的年、月、日前各有填空底線
    let owner = ps.iter().find(|p| p.text().contains("原負責人")).unwrap();
    assert!(owner.runs.last().unwrap().underline, "{:?}", owner.runs);
    let date = ps.iter().find(|p| compact(&p.text()) == "中華民國年月日").expect("日期行順序錯誤");
    assert_eq!(date.runs.iter().filter(|r| r.underline).count(), 3, "{:?}", date.runs);

    // 表格標籤沒有缺字
    let table = l
        .blocks
        .iter()
        .find_map(|b| if let Block::Table(t) = b { Some(t) } else { None })
        .expect("沒有偵測到表格");
    let cells: Vec<String> = table
        .rows
        .iter()
        .flat_map(|r| &r.cells)
        .map(|c| compact(&c.paragraphs.iter().map(|p| p.text()).collect::<String>()))
        .collect();
    for label in [
        "公司行號",
        "營業地址",
        "公司統一編號",
        "箱號",
        "聯絡電話",
        "立切結書人新負責人",
        "立切結書人原負責人",
        "身分證字號",
        "住址",
        "（簽名）（印鑑）",
        "（印鑑）",
    ] {
        assert!(cells.iter().any(|c| c == label), "表格缺少「{label}」：{cells:?}");
    }
    assert_eq!(cells.iter().filter(|c| *c == "住址").count(), 2);
}
