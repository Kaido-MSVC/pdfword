//! 開發用：列出 PDF 的版面分析結果
//! 用法：cargo run --example dump -- <檔案.pdf>

use pdfword::model::Block;
use std::sync::atomic::AtomicBool;

fn main() {
    let path = std::env::args().nth(1).expect("用法：dump <檔案.pdf>");
    let dll_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    pdfword::extract::init(&dll_dir).unwrap();
    let doc = pdfword::extract::extract_document(path.as_ref(), &AtomicBool::new(false), |_, _| {}).unwrap();
    let body = pdfword::layout::body_size(&doc.pages);
    println!("本文字級 {body}");
    for (i, p) in doc.pages.iter().enumerate() {
        println!("=== 第 {} 頁 {}x{}：{} 字、{} 圖、{} 線", i + 1, p.width, p.height, p.glyphs.len(), p.images.len(), p.rulings.len());
        if std::env::var("DUMP_LINES").is_ok() {
            debug_lines(p);
        }
        if std::env::var("DUMP_RULINGS").is_ok() {
            for r in &p.rulings {
                println!("  線 ({:.1},{:.1})-({:.1},{:.1})", r.x0, r.y0, r.x1, r.y1);
            }
        }
        let l = pdfword::layout::analyze(p, body);
        for b in &l.blocks {
            match b {
                Block::Paragraph(p) => println!(
                    "[P{} {:?} 左{:.0} 首{:.0} 前{:.0} {:.1}pt] {}",
                    p.heading.map(|h| format!(" H{h}")).unwrap_or_default(),
                    p.align, p.indent_left, p.indent_first, p.space_before, p.size,
                    p.text()
                ),
                Block::Table(t) => {
                    println!("[表格 {}x{}]", t.rows.len(), t.col_widths.len());
                    for r in &t.rows {
                        let cells: Vec<String> = r
                            .cells
                            .iter()
                            .map(|c| format!("{}{:?}:{}", c.col_span, c.vmerge, c.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("/")))
                            .collect();
                        println!("   | {}", cells.join(" | "));
                    }
                }
                Block::Image(img) => println!("[圖片 {}x{}px @ {:?}]", img.px_w, img.px_h, img.bbox),
            }
        }
    }
}

#[allow(dead_code)]
fn debug_lines(p: &pdfword::model::PageData) {
    let refs: Vec<&pdfword::model::Glyph> = p.glyphs.iter().collect();
    for l in pdfword::layout::lines::build_lines(&refs) {
        println!("  行 {:?} base={:.1} size={:.1} {}", l.bbox, l.baseline, l.size, l.text());
    }
}
