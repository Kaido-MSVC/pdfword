//! 以框線（水平/垂直線段）偵測表格格網與合併儲存格。

use crate::model::{Rect, Ruling};

#[derive(Debug, Clone, PartialEq)]
pub struct GridCell {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
}

impl GridCell {
    pub fn rect(&self, g: &TableGrid) -> Rect {
        Rect::new(
            g.xs[self.col],
            g.ys[self.row],
            g.xs[self.col + self.col_span],
            g.ys[self.row + self.row_span],
        )
    }
}

#[derive(Debug, Clone)]
pub struct TableGrid {
    pub bbox: Rect,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub cells: Vec<GridCell>,
}

impl TableGrid {
    pub fn rows(&self) -> usize {
        self.ys.len() - 1
    }
    pub fn cols(&self) -> usize {
        self.xs.len() - 1
    }
}

/// 位置容差（pt）
const TOL: f32 = 2.5;
/// 單頁框線超過此數量就不做表格偵測（避免網底花紋等耗時）
const MAX_RULINGS: usize = 4000;

pub fn detect(rulings: &[Ruling], page: Rect) -> Vec<TableGrid> {
    if rulings.len() > MAX_RULINGS {
        return Vec::new();
    }
    let (h, v): (Vec<Ruling>, Vec<Ruling>) = rulings.iter().partition(|r| r.is_horizontal());
    let h = merge_collinear(h, true);
    let v = merge_collinear(v, false);
    if h.len() < 2 || v.len() < 2 {
        return Vec::new();
    }

    // 以交點建立連通元件
    let n = h.len() + v.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    for (i, a) in h.iter().enumerate() {
        for (j, b) in v.iter().enumerate() {
            if crosses(a, b) {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, h.len() + j));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, (Vec<Ruling>, Vec<Ruling>)> = Default::default();
    for (i, r) in h.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().0.push(*r);
    }
    for (j, r) in v.iter().enumerate() {
        let root = find(&mut parent, h.len() + j);
        groups.entry(root).or_default().1.push(*r);
    }

    let mut out = Vec::new();
    for (_, (gh, gv)) in groups {
        if let Some(t) = build_grid(&gh, &gv) {
            // 單一大框（例如整頁外框）不視為表格
            if t.cells.len() < 2 || t.bbox.area() > page.area() * 0.95 {
                continue;
            }
            out.push(t);
        }
    }
    out
}

fn crosses(h: &Ruling, v: &Ruling) -> bool {
    v.x0 >= h.x0 - TOL && v.x0 <= h.x1 + TOL && h.y0 >= v.y0 - TOL && h.y0 <= v.y1 + TOL
}

/// 合併同一直線上相接或重疊的線段
fn merge_collinear(mut rs: Vec<Ruling>, horizontal: bool) -> Vec<Ruling> {
    let key = |r: &Ruling| if horizontal { (r.y0, r.x0, r.x1) } else { (r.x0, r.y0, r.y1) };
    rs.sort_by(|a, b| {
        let (pa, sa, _) = key(a);
        let (pb, sb, _) = key(b);
        pa.total_cmp(&pb).then(sa.total_cmp(&sb))
    });
    let mut out: Vec<(f32, f32, f32)> = Vec::new();
    for r in &rs {
        let (p, s, e) = key(r);
        if let Some(last) = out.iter_mut().rev().find(|l| (l.0 - p).abs() <= 1.0 && s <= l.2 + TOL) {
            last.2 = last.2.max(e);
            continue;
        }
        out.push((p, s, e));
    }
    out.into_iter()
        .filter(|(_, s, e)| e - s > 3.0)
        .map(|(p, s, e)| if horizontal { Ruling::horizontal(p, s, e) } else { Ruling::vertical(p, s, e) })
        .collect()
}

/// 把接近的位置合為一個（取平均）
fn cluster(mut vals: Vec<f32>, min_gap: f32) -> Vec<f32> {
    vals.sort_by(f32::total_cmp);
    let mut out: Vec<(f32, usize)> = Vec::new();
    for v in vals {
        match out.last_mut() {
            Some((sum, n)) if v - *sum / *n as f32 <= min_gap => {
                *sum += v;
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out.into_iter().map(|(s, n)| s / n as f32).collect()
}

fn build_grid(h: &[Ruling], v: &[Ruling]) -> Option<TableGrid> {
    let xs = cluster(v.iter().map(|r| r.x0).collect(), 4.0);
    let ys = cluster(h.iter().map(|r| r.y0).collect(), 4.0);
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    let (rows, cols) = (ys.len() - 1, xs.len() - 1);

    let has_v = |x: f32, ya: f32, yb: f32| {
        let mid = (ya + yb) / 2.0;
        v.iter().any(|r| (r.x0 - x).abs() <= TOL + 1.0 && r.y0 <= mid && r.y1 >= mid)
    };
    let has_h = |y: f32, xa: f32, xb: f32| {
        let mid = (xa + xb) / 2.0;
        h.iter().any(|r| (r.y0 - y).abs() <= TOL + 1.0 && r.x0 <= mid && r.x1 >= mid)
    };

    let mut used = vec![vec![false; cols]; rows];
    let mut cells = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if used[r][c] {
                continue;
            }
            let mut cs = 1;
            while c + cs < cols && !used[r][c + cs] && !has_v(xs[c + cs], ys[r], ys[r + 1]) {
                cs += 1;
            }
            let mut rs = 1;
            while r + rs < rows
                && (c..c + cs).all(|k| !used[r + rs][k] && !has_h(ys[r + rs], xs[k], xs[k + 1]))
            {
                rs += 1;
            }
            for rr in r..r + rs {
                for cc in c..c + cs {
                    used[rr][cc] = true;
                }
            }
            cells.push(GridCell { row: r, col: c, row_span: rs, col_span: cs });
        }
    }

    // 外框必須大致完整：四邊至少都有框線覆蓋一半以上
    let bbox = Rect::new(xs[0], ys[0], *xs.last().unwrap(), *ys.last().unwrap());
    let left = (0..rows).filter(|&r| has_v(xs[0], ys[r], ys[r + 1])).count();
    let right = (0..rows).filter(|&r| has_v(xs[cols], ys[r], ys[r + 1])).count();
    let top = (0..cols).filter(|&c| has_h(ys[0], xs[c], xs[c + 1])).count();
    let bottom = (0..cols).filter(|&c| has_h(ys[rows], xs[c], xs[c + 1])).count();
    if left * 2 < rows || right * 2 < rows || top * 2 < cols || bottom * 2 < cols {
        return None;
    }
    Some(TableGrid { bbox, xs, ys, cells })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Rect {
        Rect::new(0.0, 0.0, 600.0, 800.0)
    }

    /// 由格線座標產生完整框線
    fn full_grid(xs: &[f32], ys: &[f32]) -> Vec<Ruling> {
        let mut out = Vec::new();
        for &y in ys {
            out.push(Ruling::horizontal(y, xs[0], *xs.last().unwrap()));
        }
        for &x in xs {
            out.push(Ruling::vertical(x, ys[0], *ys.last().unwrap()));
        }
        out
    }

    #[test]
    fn simple_3x2() {
        let t = detect(&full_grid(&[50.0, 150.0, 250.0, 350.0], &[100.0, 120.0, 140.0]), page());
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].rows(), t[0].cols()), (2, 3));
        assert_eq!(t[0].cells.len(), 6);
    }

    #[test]
    fn merged_header_cell() {
        let mut r = vec![
            Ruling::horizontal(100.0, 50.0, 250.0),
            Ruling::horizontal(120.0, 50.0, 250.0),
            Ruling::horizontal(140.0, 50.0, 250.0),
            Ruling::vertical(50.0, 100.0, 140.0),
            Ruling::vertical(250.0, 100.0, 140.0),
        ];
        // 中間的直線只畫在第二列 → 第一列合併成一格
        r.push(Ruling::vertical(150.0, 120.0, 140.0));
        let t = detect(&r, page());
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].cells[0], GridCell { row: 0, col: 0, row_span: 1, col_span: 2 });
        assert_eq!(t[0].cells.len(), 3);
    }

    #[test]
    fn vertical_merge() {
        // 左欄兩列合併：左欄中間沒有水平線
        let r = vec![
            Ruling::horizontal(100.0, 50.0, 250.0),
            Ruling::horizontal(120.0, 150.0, 250.0),
            Ruling::horizontal(140.0, 50.0, 250.0),
            Ruling::vertical(50.0, 100.0, 140.0),
            Ruling::vertical(150.0, 100.0, 140.0),
            Ruling::vertical(250.0, 100.0, 140.0),
        ];
        let t = detect(&r, page());
        assert_eq!(t[0].cells[0], GridCell { row: 0, col: 0, row_span: 2, col_span: 1 });
    }

    #[test]
    fn single_box_and_lone_lines_are_not_tables() {
        assert!(detect(&full_grid(&[50.0, 300.0], &[100.0, 200.0]), page()).is_empty());
        let lines = vec![Ruling::horizontal(100.0, 0.0, 500.0), Ruling::horizontal(300.0, 0.0, 500.0)];
        assert!(detect(&lines, page()).is_empty());
    }

    #[test]
    fn cell_by_cell_borders_merge() {
        // 每格各自畫邊框（線段重疊、分段）
        let mut r = Vec::new();
        for (x0, x1) in [(50.0, 150.0), (150.0, 250.0)] {
            for (y0, y1) in [(100.0, 120.0), (120.0, 140.0)] {
                r.push(Ruling::horizontal(y0, x0, x1));
                r.push(Ruling::horizontal(y1, x0, x1));
                r.push(Ruling::vertical(x0, y0, y1));
                r.push(Ruling::vertical(x1, y0, y1));
            }
        }
        let t = detect(&r, page());
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].cells.len(), 4);
    }
}
