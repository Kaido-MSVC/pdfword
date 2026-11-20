//! PDFium → PageData。所有 PDFium 呼叫都集中在這裡。

use crate::imgxform::{self, Placement};
use crate::layout::figures;
use crate::layout::lines::is_cjk;
use crate::model::{Glyph, ImageItem, PageData, Rect, Ruling, VectorShape};
use image::DynamicImage;
use pdfium_render::prelude::*;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());
/// PDFium 不是執行緒安全的；以頁為單位序列化，讓多檔轉換時版面分析與輸出仍可平行
static GATE: Mutex<()> = Mutex::new(());

/// 圖片最長邊上限（px），避免 DOCX 過大
const MAX_IMAGE_PX: u32 = 3000;
/// 向量圖點陣化倍率（2 = 144 DPI）
const FIGURE_SCALE: f32 = 2.0;

#[derive(Debug)]
pub enum ExtractError {
    Password,
    Cancelled,
    Other(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Password => write!(f, "PDF 有密碼保護，無法開啟"),
            ExtractError::Cancelled => write!(f, "已取消"),
            ExtractError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl From<PdfiumError> for ExtractError {
    fn from(e: PdfiumError) -> Self {
        match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => ExtractError::Password,
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
                ExtractError::Other("PDF 格式錯誤或檔案已損毀".into())
            }
            other => ExtractError::Other(format!("PDFium 錯誤：{other:?}")),
        }
    }
}

/// 載入 exe 同目錄的 pdfium.dll
pub fn init(dir: &Path) -> Result<(), String> {
    // 套件內部先設定全域繫結，整段初始化必須保持互斥。
    let _init = INIT
        .lock()
        .map_err(|_| "PDFium 初始化鎖已失效".to_owned())?;
    if PDFIUM.get().is_some() {
        return Ok(());
    }
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir))
        .map_err(|e| format!("無法載入 pdfium.dll：{e:?}"))?;
    PDFIUM.get_or_init(|| Pdfium::new(bindings));
    Ok(())
}

pub struct DocInfo {
    pub pages: Vec<PageData>,
}

/// 擷取整份文件。`on_page(已完成頁數, 總頁數)` 用於回報進度。
pub fn extract_document(
    path: &Path,
    cancel: &AtomicBool,
    mut on_page: impl FnMut(usize, usize),
) -> Result<DocInfo, ExtractError> {
    let pdfium = PDFIUM.get().ok_or_else(|| ExtractError::Other("PDFium 尚未初始化".into()))?;
    // 以位元組載入，避免 PDFium 無法開啟含中文等非 ASCII 字元的路徑
    let bytes = std::fs::read(path).map_err(|e| ExtractError::Other(format!("無法讀取檔案：{e}")))?;

    let doc = {
        let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
        pdfium.load_pdf_from_byte_vec(bytes, None)?
    };
    let total = {
        let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
        doc.pages().len() as usize
    };
    let mut pages = Vec::with_capacity(total);
    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
            drop(doc);
            return Err(ExtractError::Cancelled);
        }
        let (mut data, raw_images) = {
            let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
            let mut page = doc.pages().get(i as PdfPageIndex)?;
            // 取圖片像素時會暫時改物件矩陣；不要讓 pdfium-render 重新產生頁面內容串流
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            extract_page(&page, &doc)?
        };
        // PNG 編碼在鎖外進行
        data.images = raw_images.into_iter().filter_map(to_image_item).collect();
        pages.push(data);
        on_page(i + 1, total);
    }
    {
        let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
        drop(doc);
    }
    Ok(DocInfo { pages })
}

/// 子集字型名稱去除 `ABCDEF+` 字首
pub fn clean_font_name(name: &str) -> String {
    let n = match name.split_once('+') {
        Some((p, rest)) if p.len() == 6 && p.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    };
    n.trim().to_string()
}

fn name_is_bold(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    ["bold", "heavy", "black", "semibold", "demi", ",bold"].iter().any(|k| l.contains(k))
}

fn name_is_italic(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    l.contains("italic") || l.contains("oblique")
}

/// 尚未編碼的圖片；`placement` 為 None 表示像素已是頁面上看到的樣子（例如向量圖點陣化結果）
struct RawImage {
    img: DynamicImage,
    bbox: Rect,
    placement: Option<Placement>,
}

/// 回傳頁面資料與尚未編碼的圖片（images 欄位留空，由呼叫端在鎖外編碼）
fn extract_page(page: &PdfPage, doc: &PdfDocument) -> Result<(PageData, Vec<RawImage>), ExtractError> {
    let w = page.width().value;
    let h = page.height().value;
    let flip = |y: f32| h - y;
    let mut data = PageData { width: w, height: h, ..Default::default() };

    // 文字
    let text = page.text()?;
    let mut pending_space = false;
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        if ch.is_control() || ch.is_whitespace() {
            // PDF 內容本身的空白（非 PDFium 自動產生）要保留，中日韓文字間也一樣
            if ch == ' ' || ch == '　' {
                pending_space |= !c.is_generated().unwrap_or(true);
            }
            continue;
        }
        let Ok(b) = c.loose_bounds() else { continue };
        let (ox, oy) = c.origin().unwrap_or((b.left(), b.bottom()));
        let size = c.scaled_font_size().value;
        let mut bbox = Rect::new(b.left().value, flip(b.top().value), b.right().value, flip(b.bottom().value));
        // 部分字型（如 DFKaiShu 內嵌子集）回報的字框比字級大數倍或偏離原點，改由原點與字級推算
        if size > 0.5
            && (bbox.width() > 1.3 * size || bbox.height() > 1.6 * size || (bbox.x0 - ox.value).abs() > 0.25 * size)
        {
            let adv = if is_cjk(ch) { size } else { bbox.width().min(0.6 * size) };
            let base = flip(oy.value);
            bbox = Rect::new(ox.value, base - 0.88 * size, ox.value + adv, base + 0.12 * size);
        }
        let font = clean_font_name(&c.font_name());
        let weight_bold = matches!(
            c.font_weight(),
            Some(PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900)
        ) || matches!(c.font_weight(), Some(PdfFontWeight::Custom(v)) if v >= 600)
            // 填滿再描邊是 Word 等軟體對沒有粗體字重的字型模擬粗體的方式
            || matches!(c.render_mode(), Ok(PdfPageTextRenderMode::FilledThenStroked));
        let color = c.fill_color().map(|k| [k.red(), k.green(), k.blue()]).unwrap_or([0, 0, 0]);
        data.glyphs.push(Glyph {
            ch,
            bbox,
            baseline: flip(oy.value),
            size,
            bold: weight_bold || c.font_is_bold_reenforced() || name_is_bold(&font),
            italic: c.font_is_italic() || name_is_italic(&font),
            font,
            color,
            space_before: std::mem::take(&mut pending_space),
            underline: false,
        });
    }

    // 圖片、框線、向量圖
    let mut shapes = Vec::new();
    let mut images: Vec<RawImage> = Vec::new();
    for mut obj in page.objects().iter() {
        walk_object(&mut obj, doc, h, &[], &mut data, &mut images, &mut shapes);
    }

    // 圖表等向量圖：整區點陣化，區內文字與圖片併入圖中
    let page_rect = Rect::new(0.0, 0.0, w, h);
    let regions = figures::detect(&shapes, &data.glyphs, &data.rulings, page_rect);
    if !regions.is_empty() {
        let cfg = PdfRenderConfig::new().set_target_width(((w * FIGURE_SCALE) as i32).max(1));
        if let Ok(img) = page.render_with_config(&cfg).and_then(|b| b.as_image()) {
            let inside = |rx: f32, ry: f32| regions.iter().any(|r| r.contains_point(rx, ry));
            data.glyphs.retain(|g| !inside(g.bbox.cx(), g.bbox.cy()));
            data.rulings.retain(|l| !inside((l.x0 + l.x1) / 2.0, (l.y0 + l.y1) / 2.0));
            images.retain(|i| !inside(i.bbox.cx(), i.bbox.cy()));
            let sx = img.width() as f32 / w;
            let sy = img.height() as f32 / h;
            for r in &regions {
                let (x, y) = ((r.x0 * sx).max(0.0) as u32, (r.y0 * sy).max(0.0) as u32);
                let cw = ((r.width() * sx) as u32).min(img.width().saturating_sub(x));
                let ch = ((r.height() * sy) as u32).min(img.height().saturating_sub(y));
                if cw > 0 && ch > 0 {
                    images.push(RawImage { img: img.crop_imm(x, y, cw, ch), bbox: *r, placement: None });
                }
            }
        }
    }
    Ok((data, images))
}

/// 依序套用矩陣（由內而外）
fn apply_chain(chain: &[PdfMatrix], x: PdfPoints, y: PdfPoints) -> (PdfPoints, PdfPoints) {
    chain.iter().fold((x, y), |(x, y), m| m.apply_to_points(x, y))
}

/// `parents`：外層表單 XObject 的矩陣（由內而外）；表單內物件的座標需再經這些矩陣轉到頁面
fn walk_object(
    obj: &mut PdfPageObject,
    doc: &PdfDocument,
    h: f32,
    parents: &[PdfMatrix],
    data: &mut PageData,
    images: &mut Vec<RawImage>,
    shapes: &mut Vec<VectorShape>,
) {
    if let Some(form) = obj.as_x_object_form_object() {
        if parents.len() < 8 {
            let mut chain = vec![form.matrix().unwrap_or(PdfMatrix::IDENTITY)];
            chain.extend_from_slice(parents);
            for mut child in form.iter() {
                walk_object(&mut child, doc, h, &chain, data, images, shapes);
            }
        }
        return;
    }
    let Ok(q) = obj.bounds() else { return };
    let mut r = q.to_rect();
    for m in parents {
        r = r.transform(*m);
    }
    let bbox = Rect::new(r.left().value, h - r.top().value, r.right().value, h - r.bottom().value);

    if let Some(img) = obj.as_image_object_mut() {
        if bbox.width() < 4.0 || bbox.height() < 4.0 {
            return;
        }
        // 圖片空間的單位正方形經物件矩陣與外層表單矩陣映到頁面；PDF 圖片第 0 列在 (0, 1)
        let mut chain = vec![img.matrix().unwrap_or(PdfMatrix::IDENTITY)];
        chain.extend_from_slice(parents);
        let pt = |x: f32, y: f32| {
            let (px, py) = apply_chain(&chain, PdfPoints::new(x), PdfPoints::new(y));
            (px.value, h - py.value)
        };
        let (o, right, down) = (pt(0.0, 1.0), pt(1.0, 1.0), pt(0.0, 0.0));
        let placement = Placement { origin: o, cx: (right.0 - o.0, right.1 - o.1), ry: (down.0 - o.0, down.1 - o.1) };
        if let Some(di) = upright_pixels(img, doc) {
            images.push(RawImage { img: di, bbox, placement: Some(placement) });
        }
        return;
    }

    if let Some(path) = obj.as_path_object() {
        let stroked = path.is_stroked().unwrap_or(false);
        let filled = !matches!(path.fill_mode(), Ok(PdfPathFillMode::None) | Err(_));
        if !stroked && !filled {
            return;
        }
        // 白色填滿且無框線（底色）不處理
        let white = |c: Result<PdfColor, PdfiumError>| {
            c.map(|c| c.alpha() == 0 || (c.red() > 245 && c.green() > 245 && c.blue() > 245)).unwrap_or(false)
        };
        if !stroked && white(obj.fill_color()) {
            return;
        }

        let mut segs: Vec<(f32, f32, f32, f32)> = Vec::new();
        let mut curve = false;
        let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
        // segments() 是路徑本身的座標，須套用物件矩陣與外層表單矩陣
        let mut chain = vec![path.matrix().unwrap_or(PdfMatrix::IDENTITY)];
        chain.extend_from_slice(parents);
        for s in path.segments().iter() {
            let (px, py) = apply_chain(&chain, s.x(), s.y());
            let p = (px.value, h - py.value);
            match s.segment_type() {
                PdfPathSegmentType::MoveTo => start = p,
                PdfPathSegmentType::LineTo => segs.push((cur.0, cur.1, p.0, p.1)),
                PdfPathSegmentType::BezierTo => curve = true,
                PdfPathSegmentType::Unknown => {}
            }
            cur = p;
            if s.is_close() {
                if (cur.0 - start.0).abs() > 0.01 || (cur.1 - start.1).abs() > 0.01 {
                    segs.push((cur.0, cur.1, start.0, start.1));
                }
                cur = start;
            }
        }
        let axis = segs.iter().all(|&(x0, y0, x1, y1)| (x0 - x1).abs() < 0.8 || (y0 - y1).abs() < 0.8);
        if curve || !axis {
            // 小圓點（HTML/Word 轉出的項目符號常畫成圖形）→ 轉為「•」字元
            let (bw, bh) = (bbox.width(), bbox.height());
            if filled && (2.0..=9.0).contains(&bw) && (2.0..=9.0).contains(&bh) && (bw - bh).abs() < 1.5 {
                let size = bh * 2.5;
                data.glyphs.push(Glyph {
                    ch: '•',
                    bbox,
                    baseline: bbox.cy() + 0.3 * size,
                    size,
                    // 不指定字型：Word 的 Symbol 字型沒有 U+2022
                    font: String::new(),
                    bold: false,
                    italic: false,
                    color: obj.fill_color().map(|k| [k.red(), k.green(), k.blue()]).unwrap_or([0, 0, 0]),
                    space_before: false,
                    underline: false,
                });
                return;
            }
            shapes.push(VectorShape { bbox, rect_only: false });
            return;
        }
        let stroke_visible = stroked && !white(obj.stroke_color());
        if stroke_visible {
            for (x0, y0, x1, y1) in segs {
                if (y0 - y1).abs() < 0.8 && (x1 - x0).abs() > 3.0 {
                    data.rulings.push(Ruling::horizontal(y0, x0, x1));
                } else if (x0 - x1).abs() < 0.8 && (y1 - y0).abs() > 3.0 {
                    data.rulings.push(Ruling::vertical(x0, y0, y1));
                }
            }
        } else if filled {
            // 細長填色矩形常用來畫表格框線
            if bbox.height() <= 2.5 && bbox.width() > 5.0 {
                data.rulings.push(Ruling::horizontal(bbox.cy(), bbox.x0, bbox.x1));
            } else if bbox.width() <= 2.5 && bbox.height() > 5.0 {
                data.rulings.push(Ruling::vertical(bbox.cx(), bbox.y0, bbox.y1));
            } else {
                shapes.push(VectorShape { bbox, rect_only: true });
                if let Ok(c) = obj.fill_color() {
                    data.fills.push((bbox, [c.red(), c.green(), c.blue()]));
                }
            }
        }
    }
}

/// 取得套用 SMask／Mask／色鍵遮罩後的直立像素（含 alpha）。
///
/// pdfium-render 的 `get_raw_image` 不套用遮罩；`get_processed_image` 雖套用遮罩，
/// 但會把矩陣中負的 a／d 翻正（丟掉鏡射），且旋轉時輸出的是外框大小的點陣圖。
/// 因此暫時把物件矩陣換成直立的「寬 × 高」，取得遮罩後的直立像素，再還原矩陣；
/// 旋轉與鏡射改由 imgxform 依原矩陣處理。尺寸在 PDFium 解碼時就限制在 MAX_IMAGE_PX 內。
fn upright_pixels(img: &mut PdfPageImageObject, doc: &PdfDocument) -> Option<DynamicImage> {
    let processed = (|| {
        let (w, h) = (img.width().ok()?, img.height().ok()?);
        if w <= 0 || h <= 0 {
            return None;
        }
        let s = (MAX_IMAGE_PX as f32 / w.max(h) as f32).min(1.0);
        let (tw, th) = (((w as f32 * s).round() as i32).max(1), ((h as f32 * s).round() as i32).max(1));
        let original = img.matrix().ok()?;
        img.reset_matrix(PdfMatrix::new(tw as f32, 0.0, 0.0, th as f32, 0.0, 0.0)).ok()?;
        let r = img.get_processed_image_with_size(doc, tw, th);
        // 一定要還原，否則後續整頁點陣化（向量圖區域）會畫錯
        let _ = img.reset_matrix(original);
        r.ok()
    })();
    // 退回原始像素（無遮罩）總比整張圖遺失好
    processed.or_else(|| img.get_raw_image().ok())
}

fn to_image_item(raw: RawImage) -> Option<ImageItem> {
    let RawImage { img, bbox, placement } = raw;
    let img = if img.width().max(img.height()) > MAX_IMAGE_PX {
        img.resize(MAX_IMAGE_PX, MAX_IMAGE_PX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    // 統一轉成 RGBA8 再編碼 PNG（docx-rs 只接受 PNG）
    let mut rgba = img.to_rgba8();
    if let Some(p) = placement {
        rgba = imgxform::apply(rgba, &p, (bbox.x0, bbox.y0, bbox.x1, bbox.y1), MAX_IMAGE_PX)?;
    }
    let rgba = DynamicImage::ImageRgba8(rgba);
    let mut buf = Vec::new();
    rgba.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).ok()?;
    Some(ImageItem { bbox, png: buf, px_w: rgba.width(), px_h: rgba.height() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_prefix_removed() {
        assert_eq!(clean_font_name("ABCDEF+MingLiU"), "MingLiU");
        assert_eq!(clean_font_name("Arial+Bold"), "Arial+Bold");
        assert!(name_is_bold("Arial,Bold"));
        assert!(name_is_italic("TimesNewRoman-Italic"));
    }
}
