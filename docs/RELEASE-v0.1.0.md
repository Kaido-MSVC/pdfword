# PDF 轉 Word v0.1.0

首次正式公開發布，適用於 Windows x64。繁體中文桌面介面，可批次將含文字層的 PDF 轉換成可編輯的 DOCX。

## 下載與使用

- `pdfword-0.1.0-portable-x64.zip`：解壓後執行 `pdfword.exe`，請保留同資料夾的 `pdfium.dll` 及授權文件。
- `pdfword-0.1.0-x64.msi`：全機安裝版，包含繁中安裝精靈、安裝位置選擇與開始功能表捷徑，需要系統管理員權限。
- `SHA256SUMS.txt`：上述兩個附件的 SHA-256。PowerShell 可使用 `Get-FileHash <下載檔案> -Algorithm SHA256` 核對。

執行檔採靜態 CRT，不需另外安裝 VC++ 執行階段。程式及 MSI 均未數位簽章，Windows 可能顯示未知發行者或 SmartScreen 提示。

## 功能與修正

- 批次佇列、同名檔處理、取消、進度、深淺色模式與資料夾設定記憶。
- 重建文字段落、標題、圖片、多欄閱讀順序及有框線的原生 Word 表格。
- 移除重複頁首頁尾，接回部分跨頁段落。
- 修正異常字框造成的行序錯亂及表格缺字、表單填空底線消失、模擬粗體遺失，以及部分中文縮排誤判。

## 驗證範圍

沿用開發階段的驗收紀錄：35 個單元測試、2 個批次整合測試及 1 個公文回歸測試通過；有 PDFium 時三個整合／回歸測試均有實際執行紀錄，缺 DLL 的警告與強制失敗模式也已驗證。測試文件曾以 Word 2019 開啟並進行版面比對。

發布檢查已核對完整 Git 歷史的公開檔案、來源及授權，逐檔比對 Portable 與 MSI 的 20 個負載檔案，並核對版本與 SHA-256。本次沒有重跑完整轉換、邊界及版面驗收。

## 已知限制

- 採流式重排，Word 會重新計算分頁，不能保證與所有原 PDF 完全一致；部分表格字距、窄欄換行及標題定位方式可能不同。
- 不含 OCR；掃描 PDF 僅能保留圖片。密碼保護 PDF 無法轉換。
- 無框線表格以段落／Tab 呈現，向量圖轉為圖片；PDF 頁面旋轉尚未處理。
- MSI 已檢查封裝內容，但沒有實際安裝、解除安裝及升級驗收紀錄；檔案總管手動拖放也未取得人工驗收紀錄。
- 這是 v0.1.0 的正式 Release，不表示已涵蓋所有 PDF 格式與版面。

## 授權

專案程式碼依 [MIT](https://github.com/Kaido-MSVC/pdfword/blob/v0.1.0/LICENSE) 授權；第三方元件保留各自授權，完整條文隨附件的 `licenses` 資料夾提供。Slint 1.18.1 採 Royalty-free License 2.0，程式「說明 → 關於」包含 AboutSlint。PDFium 固定為 chromium/7881，透過 pdfium-render 0.9.4 使用。

官方空白公文測試資料的來源及使用條件見 [測試資料來源](https://github.com/Kaido-MSVC/pdfword/blob/v0.1.0/tests/fixtures/README.md)。本 Release 不包含私人文件、備份、開發對話、建置快取或大型開發封存。
