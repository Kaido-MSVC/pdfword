//! 整合測試共用：專案根目錄與 PDFium 初始化。
//!
//! 找不到 DLL 時測試會略過但仍顯示 ok；設定 `PDFWORD_REQUIRE_PDFIUM=1` 可改為直接失敗。

use std::io::Write;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 載入 vendor/pdfium/pdfium.dll。找不到時印出警告並回傳 false，呼叫端應直接 return。
/// `test`：要略過的測試名稱，顯示在警告中。
pub fn init_pdfium(test: &str) -> bool {
    let dir = root().join("vendor").join("pdfium");
    let dll = dir.join("pdfium.dll");
    if !dll.exists() {
        let msg = format!("找不到 {}，測試「{test}」沒有執行（請先執行 scripts/fetch-pdfium.ps1）", dll.display());
        if std::env::var_os("PDFWORD_REQUIRE_PDFIUM").is_some() {
            panic!("{msg}");
        }
        // 直接寫入 stderr：eprintln! 會被測試框架攔截，通過的測試不會顯示
        let _ = writeln!(std::io::stderr().lock(), "\n!!!!!!!! 警告：{msg} !!!!!!!!\n");
        return false;
    }
    pdfword::extract::init(&dir).unwrap();
    // 平常被攔截；`cargo test -- --show-output` 時可看到，用來確認測試確實執行
    println!("已執行：{test}");
    true
}
