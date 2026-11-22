//! 開發用：命令列轉換
//! 用法：cargo run --example convert -- <輸入.pdf> <輸出.docx>

use pdfword::output::{Conflict, Reservations, Target};
use std::sync::atomic::AtomicBool;

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let input = args.next().ok_or("缺少輸入檔")?;
    let output = args.next().ok_or("缺少輸出檔")?;
    let dll_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium");
    pdfword::extract::init(&dll_dir)?;
    let t = std::time::Instant::now();
    // 開發用工具：明確指定輸出檔，沿用覆寫行為
    let target = Target::exact(output.as_ref(), Conflict::Overwrite);
    let (outcome, _) = pdfword::convert::convert_file(
        input.as_ref(),
        &target,
        &Reservations::default(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .map_err(|error| format!("轉換失敗：{error}"))?;
    println!(
        "完成：{} 頁，{:?}，無文字層={}",
        outcome.pages,
        t.elapsed(),
        outcome.no_text
    );
    Ok(())
}
