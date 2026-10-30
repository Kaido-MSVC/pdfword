fn main() {
    slint_build::compile("ui/app.slint").expect("Slint 編譯失敗");

    // exe 圖示與版本資訊
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("FileDescription", "PDF 轉 Word")
            .set("ProductName", "PDF 轉 Word")
            .set("OriginalFilename", "pdfword.exe")
            .set("LegalCopyright", "第三方元件授權見 licenses 資料夾")
            .set_language(0x0404); // 中文（台灣）
        res.compile().expect("Windows 資源編譯失敗");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
