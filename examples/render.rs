//! 開發用：把 PDF 第一頁渲染成 PNG
//! 用法：cargo run --example render -- <輸入.pdf> <輸出.png>

use pdfium_render::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let usage = "用法：render <輸入.pdf> <輸出.png>";
    let input = args.next().ok_or(usage)?;
    let output = args.next().ok_or(usage)?;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    let pdfium = Pdfium::new(Pdfium::bind_to_library(
        Pdfium::pdfium_platform_library_name_at_path(&dir),
    )?);
    let doc = pdfium.load_pdf_from_byte_vec(std::fs::read(input)?, None)?;
    let page = doc.pages().get(0)?;
    let img = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(800)
                .set_maximum_height(4096),
        )?
        .as_image()?;
    img.save(output)?;
    Ok(())
}
