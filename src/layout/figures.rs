//! 偵測需要點陣化的向量圖區域（圖表、示意圖）。

use super::tables;
use crate::model::{Glyph, Rect, Ruling, VectorShape};

const MAX_SHAPES: usize = 5000;
/// 區域內文字超過此字數就不點陣化（多半是有花邊的文字頁，保留可編輯文字較重要）
const MAX_GLYPHS_INSIDE: usize = 200;
/// 只有填色矩形的區域（可能只是網底色塊）允許的最多字數
const MAX_GLYPHS_INSIDE_RECTS: usize = 40;

pub fn detect(shapes: &[VectorShape], glyphs: &[Glyph], rulings: &[Ruling], page: Rect) -> Vec<Rect> {
    if shapes.is_empty() || shapes.len() > MAX_SHAPES {
        return Vec::new();
    }
    let table_boxes: Vec<Rect> = tables::detect(rulings, page).into_iter().map(|t| t.bbox).collect();
    let page_area = page.area();

    // (範圍, 圖形數, 是否只有矩形)
    let mut clusters: Vec<(Rect, usize, bool)> = shapes
        .iter()
        .filter(|s| s.bbox.area() < page_area * 0.9)
        .filter(|s| !table_boxes.iter().any(|t| t.contains_point(s.bbox.cx(), s.bbox.cy())))
        .map(|s| (s.bbox, 1, s.rect_only))
        .collect();
    // 表格外的線段（座標軸、格線）只用來連接附近圖形，不計入圖形數
    clusters.extend(
        rulings
            .iter()
            .map(|r| Rect::new(r.x0, r.y0, r.x1, r.y1))
            .filter(|r| !table_boxes.iter().any(|t| t.contains_point(r.cx(), r.cy())))
            .map(|r| (r, 0, true)),
    );

    // 反覆合併相鄰或重疊的區域
    loop {
        let mut merged = false;
        let mut i = 0;
        while i < clusters.len() {
            let mut j = i + 1;
            while j < clusters.len() {
                if clusters[i].0.intersects(&clusters[j].0, 6.0) {
                    let (r, n, ro) = clusters.swap_remove(j);
                    clusters[i].0 = clusters[i].0.union(&r);
                    clusters[i].1 += n;
                    clusters[i].2 &= ro;
                    merged = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged {
            break;
        }
    }

    clusters
        .into_iter()
        .filter(|(r, n, ro)| {
            *n >= if *ro { 4 } else { 3 }
                && r.width() >= 30.0
                && r.height() >= 30.0
                && r.area() >= page_area * 0.01
                && r.area() <= page_area * 0.9
        })
        .filter(|(r, _, ro)| {
            let inside = glyphs.iter().filter(|g| r.contains_point(g.bbox.cx(), g.bbox.cy())).count();
            inside <= if *ro { MAX_GLYPHS_INSIDE_RECTS } else { MAX_GLYPHS_INSIDE }
        })
        .map(|(r, _, _)| r)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Rect {
        Rect::new(0.0, 0.0, 600.0, 800.0)
    }

    #[test]
    fn chart_shapes_cluster_into_one_figure() {
        let shapes: Vec<VectorShape> = (0..6)
            .map(|i| VectorShape {
                bbox: Rect::new(100.0 + i as f32 * 30.0, 200.0, 125.0 + i as f32 * 30.0, 300.0),
                rect_only: true,
            })
            .collect();
        let f = detect(&shapes, &[], &[], page());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0], Rect::new(100.0, 200.0, 275.0, 300.0));
    }

    #[test]
    fn axis_line_connects_spaced_bars() {
        let shapes: Vec<VectorShape> = (0..5)
            .map(|i| VectorShape {
                bbox: Rect::new(100.0 + i as f32 * 45.0, 200.0, 130.0 + i as f32 * 45.0, 300.0),
                rect_only: true,
            })
            .collect();
        let axis = [Ruling::horizontal(300.0, 90.0, 340.0)];
        let f = detect(&shapes, &[], &axis, page());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0], Rect::new(90.0, 200.0, 340.0, 300.0));
    }

    #[test]
    fn lone_shape_and_background_ignored() {
        let shapes = vec![
            VectorShape { bbox: Rect::new(100.0, 100.0, 200.0, 200.0), rect_only: false },
            VectorShape { bbox: Rect::new(0.0, 0.0, 600.0, 800.0), rect_only: true },
        ];
        assert!(detect(&shapes, &[], &[], page()).is_empty());
    }
}
