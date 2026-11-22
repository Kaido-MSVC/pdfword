//! 依 PDF 繪製圖片時的變換矩陣（CTM）還原旋轉、鏡射與傾斜。
//!
//! 座標一律是頁面座標（pt，原點左上、y 向下）。圖片以三個量描述：
//! - `origin`：來源第 0 列第 0 欄（左上角像素）所在的位置
//! - `cx`：沿來源像素欄方向（左→右）走完整張寬度的向量
//! - `ry`：沿來源像素列方向（上→下）走完整張高度的向量
//!
//! 直立圖片為 `cx = (w, 0)`、`ry = (0, h)`。90° 倍數的旋轉與鏡射以像素重排精確還原；
//! 其他角度或傾斜無法用 DOCX 的行內圖片表達（docx-rs 0.4.22 只有整數度 rot、沒有 flipH/flipV），
//! 改為依矩陣重新取樣到外框內，外框以外為透明。

use image::{Rgba, RgbaImage};

pub type V2 = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: V2,
    pub cx: V2,
    pub ry: V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 直立、無變換
    Upright,
    /// 90° 倍數旋轉及／或鏡射：`swap` 表示寬高互換，`flip_x`/`flip_y` 表示輸出軸反向
    Dihedral { swap: bool, flip_x: bool, flip_y: bool },
    /// 任意角度或傾斜，需重新取樣
    Warp,
}

fn len(v: V2) -> f32 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

/// 判斷矩陣型態；矩陣退化（寬或高為 0）時回傳 None
pub fn classify(p: &Placement) -> Option<Kind> {
    let (lc, lr) = (len(p.cx), len(p.ry));
    if !(lc > 1e-3 && lr > 1e-3) || !lc.is_finite() || !lr.is_finite() {
        return None;
    }
    // 分量小於長度的 0.5% 視為軸向
    let axis = |v: V2, l: f32| {
        if v.1.abs() <= 0.005 * l {
            Some(true) // 水平
        } else if v.0.abs() <= 0.005 * l {
            Some(false) // 垂直
        } else {
            None
        }
    };
    match (axis(p.cx, lc), axis(p.ry, lr)) {
        (Some(true), Some(false)) => {
            let (flip_x, flip_y) = (p.cx.0 < 0.0, p.ry.1 < 0.0);
            Some(if !flip_x && !flip_y { Kind::Upright } else { Kind::Dihedral { swap: false, flip_x, flip_y } })
        }
        // 來源欄方向變成垂直：寬高互換
        (Some(false), Some(true)) => Some(Kind::Dihedral { swap: true, flip_x: p.ry.0 < 0.0, flip_y: p.cx.1 < 0.0 }),
        _ => Some(Kind::Warp),
    }
}

/// 90° 倍數旋轉／鏡射：每個像素精確搬到新位置
pub fn dihedral(src: &RgbaImage, swap: bool, flip_x: bool, flip_y: bool) -> RgbaImage {
    let (w, h) = src.dimensions();
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut out = RgbaImage::new(ow, oh);
    for (i, j, px) in src.enumerate_pixels() {
        // 未互換：輸出 x 來自欄 i、y 來自列 j；互換：x 來自列 j、y 來自欄 i
        let (x, y) = if swap { (j, i) } else { (i, j) };
        let x = if flip_x { ow - 1 - x } else { x };
        let y = if flip_y { oh - 1 - y } else { y };
        out.put_pixel(x, y, *px);
    }
    out
}

/// 任意仿射：依矩陣反推每個輸出像素在來源中的位置（雙線性內插），輸出涵蓋外框 `bbox`（x0, y0, x1, y1）。
/// `max_px` 限制輸出最長邊。
pub fn warp(src: &RgbaImage, p: &Placement, bbox: (f32, f32, f32, f32), max_px: u32) -> Option<RgbaImage> {
    let (w, h) = src.dimensions();
    let (bw, bh) = (bbox.2 - bbox.0, bbox.3 - bbox.1);
    if w == 0
        || h == 0
        || max_px == 0
        || !(bw.is_finite() && bh.is_finite() && bw > 0.0 && bh > 0.0)
    {
        return None;
    }
    // 保持來源解析度（px/pt），再限制最長邊
    let mut s = (w as f32 / len(p.cx)).max(h as f32 / len(p.ry));
    s = s.min(max_px as f32 / bw.max(bh));
    let det = p.cx.0 * p.ry.1 - p.cx.1 * p.ry.0;
    if !s.is_finite() || s <= 0.0 || !det.is_finite() || det.abs() < 1e-6 {
        return None;
    }
    // 長邊上限已用於計算倍率，再限制一次像素尺寸以吸收浮點乘除的捨入誤差。
    let output_dimension =
        |side: f32| f64::from((side * s).ceil()).clamp(1.0, f64::from(max_px)) as u32;
    let (ow, oh) = (output_dimension(bw), output_dimension(bh));
    let sample = |x: f32, y: f32| -> [f32; 4] {
        let (x, y) = (x.clamp(0.0, (w - 1) as f32), y.clamp(0.0, (h - 1) as f32));
        let (x0, y0) = ((x.floor() as u32).min(w - 1), (y.floor() as u32).min(h - 1));
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |x, y| src.get_pixel(x, y).0.map(f32::from);
        let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    };
    let mut out = RgbaImage::new(ow, oh);
    for (x, y, px) in out.enumerate_pixels_mut() {
        let q = (bbox.0 + (x as f32 + 0.5) / s - p.origin.0, bbox.1 + (y as f32 + 0.5) / s - p.origin.1);
        // q = a·cx + b·ry，a、b ∈ [0, 1) 時落在圖片內
        let a = (q.0 * p.ry.1 - q.1 * p.ry.0) / det;
        let b = (p.cx.0 * q.1 - p.cx.1 * q.0) / det;
        if (0.0..1.0).contains(&a) && (0.0..1.0).contains(&b) {
            let v = sample(a * w as f32 - 0.5, b * h as f32 - 0.5);
            *px = Rgba(v.map(|c| c.round().clamp(0.0, 255.0) as u8));
        }
    }
    Some(out)
}

/// 依矩陣把直立的來源像素轉成頁面上看到的樣子；退化矩陣回傳 None
pub fn apply(src: RgbaImage, p: &Placement, bbox: (f32, f32, f32, f32), max_px: u32) -> Option<RgbaImage> {
    match classify(p)? {
        Kind::Upright => Some(src),
        Kind::Dihedral { swap, flip_x, flip_y } => Some(dihedral(&src, swap, flip_x, flip_y)),
        Kind::Warp => warp(&src, p, bbox, max_px),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×1 圖：左紅右藍
    fn red_blue() -> RgbaImage {
        let mut i = RgbaImage::new(2, 1);
        i.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        i.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        i
    }

    fn pl(origin: V2, cx: V2, ry: V2) -> Placement {
        Placement { origin, cx, ry }
    }

    #[test]
    fn classify_cases() {
        assert_eq!(classify(&pl((0., 0.), (20., 0.), (0., 10.))), Some(Kind::Upright));
        // 水平鏡射：左上角在右邊
        assert_eq!(classify(&pl((20., 0.), (-20., 0.), (0., 10.))), Some(Kind::Dihedral { swap: false, flip_x: true, flip_y: false }));
        // 順時針 90°：欄方向朝下、列方向朝左
        assert_eq!(classify(&pl((10., 0.), (0., 20.), (-10., 0.))), Some(Kind::Dihedral { swap: true, flip_x: true, flip_y: false }));
        assert_eq!(classify(&pl((0., 0.), (14., 14.), (-7., 7.))), Some(Kind::Warp));
        assert_eq!(classify(&pl((0., 0.), (0., 0.), (0., 10.))), None);
    }

    #[test]
    fn flip_horizontal() {
        let o = dihedral(&red_blue(), false, true, false);
        assert_eq!(o.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(o.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn rotate_90_clockwise() {
        // 左紅右藍順時針轉 90° → 上紅下藍、寬 1 高 2
        let p = pl((1., 0.), (0., 2.), (-1., 0.));
        let o = apply(red_blue(), &p, (0., 0., 1., 2.), 100).unwrap();
        assert_eq!(o.dimensions(), (1, 2));
        assert_eq!(o.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(o.get_pixel(0, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn warp_45_degrees_has_transparent_corners() {
        let src = RgbaImage::from_pixel(10, 10, Rgba([0, 128, 0, 255]));
        // 菱形：左上角像素在上方頂點
        let k = 10.0 / 2f32.sqrt();
        let p = pl((k, 0.), (k, k), (-k, k));
        let o = apply(src, &p, (0., 0., 2. * k, 2. * k), 100).unwrap();
        let (w, h) = o.dimensions();
        assert_eq!(o.get_pixel(0, 0).0[3], 0, "角落應透明");
        assert_eq!(o.get_pixel(w / 2, h / 2).0, [0, 128, 0, 255]);
    }
}
