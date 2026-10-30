//! 多欄偵測與閱讀順序。
//!
//! 先以「窄」文字片段的水平覆蓋找出欄間空白（gutter），跨越 gutter 的項目（標題、
//! 表格、整寬圖片）把頁面切成上下多個帶；每個帶內依欄由左到右、欄內由上到下排序。

use crate::model::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub bbox: Rect,
    pub kind: ItemKind,
}

/// 排序結果：(項目索引, 群組編號)。同一群組代表同一欄的連續內容。
pub fn reading_order(items: &[Item]) -> Vec<(usize, usize)> {
    if items.is_empty() {
        return Vec::new();
    }
    let gutters = find_gutters(items);
    if gutters.is_empty() {
        let mut idx: Vec<usize> = (0..items.len()).collect();
        idx.sort_by(|&a, &b| {
            let (ra, rb) = (&items[a].bbox, &items[b].bbox);
            ra.y0.total_cmp(&rb.y0).then(ra.x0.total_cmp(&rb.x0))
        });
        return idx.into_iter().map(|i| (i, 0)).collect();
    }

    let crosses = |r: &Rect| gutters.iter().any(|&(gs, ge)| r.x0 < gs - 1.0 && r.x1 > ge + 1.0);
    let column_of = |r: &Rect| gutters.iter().filter(|&&(gs, _)| r.cx() > gs).count();

    let mut spanning: Vec<usize> = (0..items.len()).filter(|&i| crosses(&items[i].bbox)).collect();
    spanning.sort_by(|&a, &b| items[a].bbox.y0.total_cmp(&items[b].bbox.y0));

    let ncols = gutters.len() + 1;
    let nbands = spanning.len() + 1;
    let mut buckets: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); ncols]; nbands];
    for i in 0..items.len() {
        if spanning.contains(&i) {
            continue;
        }
        let cy = items[i].bbox.cy();
        let band = spanning.iter().filter(|&&s| items[s].bbox.cy() < cy).count();
        buckets[band][column_of(&items[i].bbox)].push(i);
    }

    let mut out = Vec::new();
    let mut group = 0;
    for (b, cols) in buckets.iter_mut().enumerate() {
        for col in cols.iter_mut() {
            if col.is_empty() {
                continue;
            }
            col.sort_by(|&a, &c| items[a].bbox.y0.total_cmp(&items[c].bbox.y0));
            out.extend(col.iter().map(|&i| (i, group)));
            group += 1;
        }
        if let Some(&s) = spanning.get(b) {
            out.push((s, group));
            group += 1;
        }
    }
    out
}

/// 回傳 gutter 的 x 範圍清單（由左到右）
fn find_gutters(items: &[Item]) -> Vec<(f32, f32)> {
    let text: Vec<&Rect> = items.iter().filter(|i| i.kind == ItemKind::Text).map(|i| &i.bbox).collect();
    if text.len() < 6 {
        return Vec::new();
    }
    let left = text.iter().map(|r| r.x0).fold(f32::MAX, f32::min);
    let right = text.iter().map(|r| r.x1).fold(f32::MIN, f32::max);
    let cw = right - left;
    if cw < 100.0 {
        return Vec::new();
    }
    let narrow: Vec<&Rect> = text.iter().copied().filter(|r| r.width() < cw * 0.6).collect();
    if narrow.len() < 6 {
        return Vec::new();
    }

    // 1pt 解析度的覆蓋直方圖
    let n = cw.ceil() as usize + 1;
    let mut hist = vec![0usize; n];
    for r in &narrow {
        let a = ((r.x0 - left).max(0.0)) as usize;
        let b = ((r.x1 - left).ceil() as usize).min(n - 1);
        for h in &mut hist[a..=b] {
            *h += 1;
        }
    }
    let tol = (narrow.len() / 20).max(1);
    let lo = (cw * 0.15) as usize;
    let hi = (cw * 0.85) as usize;

    let mut gutters = Vec::new();
    let mut x = lo;
    while x < hi {
        if hist[x] <= tol {
            let s = x;
            while x < hi && hist[x] <= tol {
                x += 1;
            }
            if x - s >= 8 {
                gutters.push((left + s as f32, left + x as f32));
            }
        }
        x += 1;
    }

    // 驗證：每欄寬度至少為內容寬 20%、至少 3 個片段，且片段寬度中位數至少為欄寬一半
    // （排除「標籤 … 數值」式排列）
    let mut bounds = vec![left];
    for &(s, e) in &gutters {
        bounds.push(s);
        bounds.push(e);
    }
    bounds.push(right);
    for k in 0..=gutters.len() {
        let (cl, cr) = (bounds[2 * k], bounds[2 * k + 1]);
        if cr - cl < cw * 0.2 {
            return Vec::new();
        }
        let mut widths: Vec<f32> = narrow
            .iter()
            .filter(|r| r.cx() >= cl && r.cx() <= cr)
            .map(|r| r.width())
            .collect();
        if widths.len() < 3 {
            return Vec::new();
        }
        widths.sort_by(f32::total_cmp);
        if widths[widths.len() / 2] < (cr - cl) * 0.5 {
            return Vec::new();
        }
    }
    gutters
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(x0: f32, y0: f32, x1: f32) -> Item {
        Item { bbox: Rect::new(x0, y0, x1, y0 + 10.0), kind: ItemKind::Text }
    }

    #[test]
    fn single_column_is_top_to_bottom() {
        let items: Vec<Item> = (0..10).map(|i| text(50.0, 100.0 + i as f32 * 14.0, 500.0)).collect();
        let order = reading_order(&items);
        assert_eq!(order.iter().map(|o| o.0).collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn two_columns_with_title() {
        let mut items = vec![text(50.0, 50.0, 500.0)]; // 整寬標題
        for i in 0..8 {
            let y = 100.0 + i as f32 * 14.0;
            items.push(text(50.0, y, 270.0)); // 左欄
            items.push(text(290.0, y, 500.0)); // 右欄
        }
        let order: Vec<usize> = reading_order(&items).into_iter().map(|o| o.0).collect();
        assert_eq!(order[0], 0); // 整寬標題在最前
        // 左欄全部排在右欄之前
        let left: Vec<usize> = (0..8).map(|i| 1 + 2 * i).collect();
        let pos = |i: usize| order.iter().position(|&o| o == i).unwrap();
        let last_left = left.iter().map(|&i| pos(i)).max().unwrap();
        let first_right = (0..8).map(|i| pos(2 + 2 * i)).min().unwrap();
        assert!(last_left < first_right);
    }

    #[test]
    fn label_value_rows_are_not_columns() {
        let mut items = Vec::new();
        for i in 0..8 {
            let y = 100.0 + i as f32 * 14.0;
            items.push(text(50.0, y, 90.0)); // 短標籤
            items.push(text(400.0, y, 500.0)); // 數值
        }
        let order = reading_order(&items);
        assert!(order.iter().all(|o| o.1 == 0));
        // 依行排列：標籤後接同行數值
        assert_eq!(order[0].0, 0);
        assert_eq!(order[1].0, 1);
    }
}
