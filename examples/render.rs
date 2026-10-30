//! 開發用：把 PDF 第一頁渲染成 PNG
//! 用法：cargo run --example render -- <輸入.pdf> <輸出.png>

use pdfium_render::prelude::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = (args.next().unwrap(), args.next().unwrap());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    let pdfium = Pdfium::new(Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir)).unwrap());
    let doc = pdfium.load_pdf_from_byte_vec(std::fs::read(input).unwrap(), None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(800)).unwrap().as_image().unwrap();
    img.save(output).unwrap();
}
