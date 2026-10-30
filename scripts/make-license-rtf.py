"""產生 installer/license.rtf（MSI 安裝精靈的授權頁）。中文以 RTF Unicode 跳脫寫入。"""
import os

TEXT = """PDF 轉 Word

本程式供個人使用及分享，按「現狀」提供，不附任何明示或默示的擔保。

本程式使用下列第三方元件，各依其授權條款使用：

• Slint 1.18.1 — Slint Royalty-free License 2.0（程式「說明 → 關於」顯示 AboutSlint）
• PDFium（chromium/7881）— BSD-3-Clause / Apache-2.0，內含 FreeType（FTL）、libjpeg-turbo、OpenJPEG、lcms2、zlib、libpng、ICU 等寬鬆授權元件
• pdfium-render、docx-rs、rayon、rfd、image、serde 等 Rust 套件 — MIT / Apache-2.0 等寬鬆授權

所有第三方授權條文的完整內容，安裝後位於程式資料夾的 licenses 子資料夾。"""


def esc(s):
    out = []
    for ch in s:
        o = ord(ch)
        if ch == '\n':
            out.append('\\par\n')
        elif ch in '\\{}':
            out.append('\\' + ch)
        elif o < 128:
            out.append(ch)
        else:
            out.append('\\u%d?' % (o if o < 32768 else o - 65536))
    return ''.join(out)


rtf = '{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0 Microsoft JhengHei;}}\\f0\\fs18\n' + esc(TEXT) + '\\par\n}'
path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'installer', 'license.rtf')
with open(path, 'w', encoding='ascii') as f:
    f.write(rtf)
