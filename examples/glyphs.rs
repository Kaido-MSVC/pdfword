//! 開發用：列出 PDF 第一頁每個字元的原始資訊（loose/tight bounds、origin、字型）
//! 用法：cargo run --example glyphs -- <輸入.pdf>

use pdfium_render::prelude::*;

fn main() {
    let input = std::env::args().nth(1).unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    let pdfium = Pdfium::new(Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir)).unwrap());
    let doc = pdfium.load_pdf_from_byte_vec(std::fs::read(input).unwrap(), None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let h = page.height().value;
    let text = page.text().unwrap();
    for c in text.chars().iter() {
        let ch = c.unicode_char().unwrap_or('?');
        let l = c.loose_bounds().ok();
        let t = c.tight_bounds().ok();
        let o = c.origin().ok();
        let f = |r: Option<PdfRect>| {
            r.map(|r| format!("[{:.1},{:.1},{:.1},{:.1}]", r.left().value, h - r.top().value, r.right().value, h - r.bottom().value))
                .unwrap_or_default()
        };
        println!(
            "{:?} gen={:?} loose={} tight={} origin={} size={:.1} font={} w={:?} fill={:?} mode={:?}",
            ch,
            c.is_generated().ok(),
            f(l),
            f(t),
            o.map(|(x, y)| format!("({:.1},{:.1})", x.value, h - y.value)).unwrap_or_default(),
            c.scaled_font_size().value,
            c.font_name(),
            c.font_weight(),
            c.fill_color().ok().map(|k| (k.red(), k.green(), k.blue())),
            c.render_mode().ok(),
        );
    }
}
