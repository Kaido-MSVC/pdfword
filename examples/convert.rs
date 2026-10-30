//! 開發用：命令列轉換
//! 用法：cargo run --example convert -- <輸入.pdf> <輸出.docx>

use std::sync::atomic::AtomicBool;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = (args.next().expect("缺少輸入檔"), args.next().expect("缺少輸出檔"));
    let dll_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    pdfword::extract::init(&dll_dir).unwrap();
    let t = std::time::Instant::now();
    match pdfword::convert::convert_file(input.as_ref(), output.as_ref(), &AtomicBool::new(false), &|_| {}) {
        Ok(o) => println!("完成：{} 頁，{:?}，無文字層={}", o.pages, t.elapsed(), o.no_text),
        Err(e) => println!("失敗：{e}"),
    }
}
