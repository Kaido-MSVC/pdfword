# PDF 轉 Word（pdfword）

Windows x64 桌面程式：把 PDF 批次轉換成可編輯的 Word（DOCX），盡量保留文字、圖片、表格與排版。繁體中文介面，不含 OCR。

使用方式見 [docs/使用說明.txt](docs/使用說明.txt)。

## 技術組成

| 項目 | 採用 | 授權 |
|---|---|---|
| GUI | Slint 1.18.1（winit + femtovg/software 渲染） | Slint Royalty-free 2.0（需顯示 AboutSlint，已放在「說明 → 關於」） |
| PDF 解析 | PDFium chromium/7881（bblanchon/pdfium-binaries，無 V8/XFA）+ pdfium-render 0.9.4 | BSD-3/Apache-2.0；MIT/Apache-2.0 |
| DOCX | docx-rs 0.4.22 | MIT |
| 平行處理 | rayon 1.12.0 | MIT/Apache-2.0 |
| 檔案對話框 | rfd 0.17.2 | MIT |
| 連結器 | LLVM lld-link 23（`.cargo/config.toml`），靜態 CRT | 建置工具，不散布 |
| 安裝程式 | WiX Toolset 7 + WixToolset.UI.wixext | 建置工具；需接受 OSMF EULA（`-acceptEula wix7`） |

除 Slint 外，所有隨程式散布的 crate 皆為 MIT/Apache-2.0/BSD/Zlib/ISC/Unicode 等寬鬆授權；`scripts/gen-notices.py` 會檢查並在發現其他授權時中止。

## 建置

需要：Rust stable-x86_64-pc-windows-msvc、Visual Studio（MSVC 程式庫與 Windows SDK）、LLVM（`C:\Program Files\LLVM\bin\lld-link.exe`）、Python 3（只用標準函式庫）、WiX 7（`wix`）。

```powershell
pwsh -File scripts\fetch-pdfium.ps1   # 下載 PDFium 到 vendor\pdfium（驗證 SHA-256）
cargo run                             # 開發執行（debug 版會從 vendor\pdfium 載入 DLL）
cargo test                            # 單元測試 + 整合測試
pwsh -File scripts\build.ps1          # 產生 dist\pdfword-<版本>-portable-x64.zip 與 dist\pdfword-<版本>-x64.msi
```

## 程式結構

```
src/extract.rs        PDFium → PageData（字元、圖片、框線、向量圖；以頁為單位加鎖）
src/layout/           版面分析：lines（行）、columns（多欄閱讀順序）、tables（框線表格）、
                      blocks（段落／對齊／標題）、figures（向量圖點陣化）
src/writer.rs         PageLayout → DOCX（流式；頁面尺寸改變才分節）
src/fonts.rs          PDF 字型 → Word 字型對應
src/convert.rs        單檔流程（暫存檔 → 改名，取消／失敗不留殘檔）
src/job.rs            批次佇列、輸出路徑規劃、取消、進度事件
src/settings.rs       %APPDATA%\pdfword\settings.json
src/main.rs、ui/      Slint 介面
installer/            WiX 7 安裝程式
scripts/              下載、建置、授權聲明、圖示與開發測試工具
```

## 開發工具

- `cargo run --example dump -- <檔案.pdf>`：列出版面分析結果（`DUMP_LINES=1` 另列出行）
- `cargo run --example convert -- <in.pdf> <out.docx>`：命令列轉換
- `scripts\word-check.ps1 <docx...>`：以 Word 開啟驗證並匯出 PDF
- `scripts\make-fixtures.ps1`：以 Edge 由 `tests\fixtures\src\*.html` 產生測試 PDF
- `scripts\ui-e2e.ps1 [-Exe <路徑>]`：以 UI Automation 操作 GUI 做端到端轉換測試
