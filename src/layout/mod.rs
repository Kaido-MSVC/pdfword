//! 版面分析：PageData（純資料）→ PageLayout（段落、表格、圖片的閱讀順序）。

pub mod blocks;
pub mod columns;
pub mod figures;
pub mod lines;
pub mod tables;
pub mod underlines;

use crate::model::{Align, Block, Cell, Glyph, PageData, PageLayout, Rect, Row, Table, VMerge};
use columns::{Item, ItemKind};
use lines::Line;
use std::collections::{HashMap, HashSet};

/// 全文件的本文字級：以字數加權的最常見字級
pub fn body_size<'a>(pages: impl IntoIterator<Item = &'a PageData>) -> f32 {
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for p in pages {
        for g in &p.glyphs {
            if !g.ch.is_whitespace() {
                *counts.entry((g.size * 2.0).round() as i32).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(k, n)| (n, -k))
        .map(|(k, _)| k as f32 / 2.0)
        .unwrap_or(11.0)
}

enum Content {
    Line(Line),
    Table(Table),
    Image(usize),
}

/// 位於頁面上下邊緣（頁首/頁尾區）
fn in_edge_zone(r: &Rect, page_height: f32) -> bool {
    r.y1 < page_height * 0.08 || r.y0 > page_height * 0.92
}

/// 比對頁首頁尾用的鍵：數字一律視為相同（頁碼、日期會變）
fn running_key(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()
}

/// 找出在多數頁面的頁首/頁尾重複出現的行（例如每頁都印的文件標題）
pub fn running_lines(pages: &[PageData]) -> HashSet<String> {
    if pages.len() < 2 {
        return HashSet::new();
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in pages {
        let edge: Vec<&Glyph> = p
            .glyphs
            .iter()
            .filter(|g| g.bbox.cy() < p.height * 0.08 || g.bbox.cy() > p.height * 0.92)
            .collect();
        let keys: HashSet<String> = lines::build_lines(&edge)
            .iter()
            .filter(|l| in_edge_zone(&l.bbox, p.height))
            .map(|l| running_key(&l.text()))
            .filter(|k| !k.is_empty())
            .collect();
        for k in keys {
            *counts.entry(k).or_default() += 1;
        }
    }
    let need = (pages.len() + 1) / 2;
    counts.into_iter().filter(|&(_, n)| n >= need.max(2)).map(|(k, _)| k).collect()
}

/// 段落跨頁時，把下一頁開頭的延續段落接回上一頁最後一段
pub fn merge_across_pages(pages: &mut [PageLayout]) {
    for i in 1..pages.len() {
        let (before, after) = pages.split_at_mut(i);
        let Some(Block::Paragraph(prev)) = before[i - 1].blocks.last_mut() else { continue };
        let Some(Block::Paragraph(next)) = after[0].blocks.first() else { continue };
        let ends_sentence = prev
            .runs
            .iter()
            .rev()
            .find_map(|r| r.text.trim_end().chars().last())
            .is_none_or(|c| "。．.！!？?：:；;」』）)".contains(c));
        let continues = prev.heading.is_none()
            && next.heading.is_none()
            && !ends_sentence
            && (prev.size - next.size).abs() < 0.5
            && next.indent_first <= 1.0
            && matches!(prev.align, Align::Left | Align::Justify)
            && matches!(next.align, Align::Left | Align::Justify);
        if continues {
            let Block::Paragraph(next) = after[0].blocks.remove(0) else { unreachable!() };
            blocks::append_runs(&mut prev.runs, &next.runs);
            prev.align = next.align;
        }
    }
}

pub fn analyze(page: &PageData, body_size: f32) -> PageLayout {
    analyze_with(page, body_size, &HashSet::new())
}

/// `running`：要略過的頁首/頁尾（見 [`running_lines`]）
pub fn analyze_with(page: &PageData, body_size: f32, running: &HashSet<String>) -> PageLayout {
    let page_rect = Rect::new(0.0, 0.0, page.width, page.height);
    let grids = tables::detect(&page.rulings, page_rect);

    // 字元分配到表格或一般文字
    let mut free: Vec<&Glyph> = Vec::new();
    let mut in_table: Vec<Vec<&Glyph>> = vec![Vec::new(); grids.len()];
    for g in &page.glyphs {
        let (cx, cy) = (g.bbox.cx(), g.bbox.cy());
        match grids.iter().position(|t| t.bbox.contains_point(cx, cy)) {
            Some(i) => in_table[i].push(g),
            None => free.push(g),
        }
    }
    let loose_rulings: Vec<_> = page
        .rulings
        .iter()
        .filter(|r| !grids.iter().any(|t| t.bbox.intersects(&Rect::new(r.x0, r.y0, r.x1, r.y1), 1.0)))
        .copied()
        .collect();
    let underlined = underlines::apply(&free, &loose_rulings);
    let free: Vec<&Glyph> = underlined.iter().collect();

    let mut contents: Vec<Content> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    for l in lines::build_lines(&free) {
        // 頁首/頁尾的頁碼與重複文字在流式文件中沒有意義
        if in_edge_zone(&l.bbox, page.height) {
            let text = l.text();
            if is_page_number(&text) || running.contains(&running_key(&text)) {
                continue;
            }
        }
        items.push(Item { bbox: l.bbox, kind: ItemKind::Text });
        contents.push(Content::Line(l));
    }
    for (grid, glyphs) in grids.iter().zip(in_table) {
        items.push(Item { bbox: grid.bbox, kind: ItemKind::Other });
        contents.push(Content::Table(build_table(grid, &glyphs, &page.fills, body_size)));
    }
    for (i, img) in page.images.iter().enumerate() {
        items.push(Item { bbox: img.bbox, kind: ItemKind::Other });
        contents.push(Content::Image(i));
    }

    let content = items.iter().map(|i| i.bbox).reduce(|a, b| a.union(&b)).unwrap_or(page_rect);
    let order = columns::reading_order(&items);

    // 每個群組的範圍（作為對齊與縮排的參考欄寬）
    let mut group_box: HashMap<usize, Rect> = HashMap::new();
    for &(i, g) in &order {
        if items[i].kind == ItemKind::Text {
            group_box.entry(g).and_modify(|r| *r = r.union(&items[i].bbox)).or_insert(items[i].bbox);
        }
    }
    // 單欄頁面以整體內容寬為欄寬，避免短段落被誤判為置中
    let single = order.iter().all(|o| o.1 == order[0].1) || group_box.len() <= 1;

    let mut contents: Vec<Option<Content>> = contents.into_iter().map(Some).collect();
    let mut blocks = Vec::new();
    let mut pending: Vec<Line> = Vec::new();
    let mut pending_group = usize::MAX;
    let mut prev_bottom: Option<f32> = None;

    let flush = |pending: &mut Vec<Line>, group: usize, blocks: &mut Vec<Block>, prev_bottom: &mut Option<f32>| {
        if pending.is_empty() {
            return;
        }
        let col = if single { content } else { group_box.get(&group).copied().unwrap_or(content) };
        let top = pending[0].bbox.y0;
        let bottom = pending.last().unwrap().bbox.y1;
        let mut ps = blocks::paragraphs(pending, col, body_size);
        if let (Some(first), Some(b)) = (ps.first_mut(), *prev_bottom) {
            // 換欄時（新內容在上一段之上）用一般段距
            first.space_before =
                if top < b { 0.6 * first.size } else { (top - b - 0.25 * first.size).clamp(0.0, 72.0) };
        }
        blocks.extend(ps.into_iter().map(Block::Paragraph));
        *prev_bottom = Some(bottom);
        pending.clear();
    };

    for (i, group) in order {
        match contents[i].take().unwrap() {
            Content::Line(l) => {
                if group != pending_group {
                    flush(&mut pending, pending_group, &mut blocks, &mut prev_bottom);
                    pending_group = group;
                }
                pending.push(l);
            }
            other => {
                flush(&mut pending, pending_group, &mut blocks, &mut prev_bottom);
                pending_group = usize::MAX;
                prev_bottom = Some(items[i].bbox.y1);
                blocks.push(match other {
                    Content::Table(t) => Block::Table(t),
                    Content::Image(k) => Block::Image(page.images[k].clone()),
                    Content::Line(_) => unreachable!(),
                });
            }
        }
    }
    flush(&mut pending, pending_group, &mut blocks, &mut prev_bottom);

    PageLayout { width: page.width, height: page.height, content, blocks }
}

/// 「12」「- 12 -」「12 / 30」「第 12 頁」「Page 12 of 30」等頁碼樣式
pub fn is_page_number(text: &str) -> bool {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    let t = t.trim_matches(|c| matches!(c, '-' | '–' | '—' | '[' | ']' | '(' | ')'));
    let digits = |s: &str| !s.is_empty() && s.len() <= 4 && s.chars().all(|c| c.is_ascii_digit());
    if digits(t) {
        return true;
    }
    if let Some((a, b)) = t.split_once('/') {
        return digits(a) && digits(b);
    }
    if let Some(rest) = t.strip_prefix('第') {
        if let Some((n, tail)) = rest.split_once('頁') {
            return digits(n) && (tail.is_empty() || tail.starts_with('共') || tail.starts_with('/'));
        }
    }
    if let Some(rest) = t.strip_prefix("page") {
        return match rest.split_once("of") {
            Some((a, b)) => digits(a) && digits(b),
            None => digits(rest),
        };
    }
    false
}

fn build_table(grid: &tables::TableGrid, glyphs: &[&Glyph], fills: &[(Rect, [u8; 3])], body_size: f32) -> Table {
    let mut rows: Vec<Row> = (0..grid.rows())
        .map(|r| Row { height: grid.ys[r + 1] - grid.ys[r], cells: Vec::new() })
        .collect();
    // (列, 欄) → 儲存格，之後依欄排序
    let mut placed: Vec<Vec<(usize, Cell)>> = vec![Vec::new(); grid.rows()];
    for gc in &grid.cells {
        let rect = gc.rect(grid);
        let inside: Vec<&Glyph> =
            glyphs.iter().copied().filter(|g| rect.contains_point(g.bbox.cx(), g.bbox.cy())).collect();
        let lines = lines::build_lines(&inside);
        let pad = Rect::new(rect.x0 + 2.0, rect.y0, rect.x1 - 2.0, rect.y1);
        let paragraphs = blocks::paragraphs(&lines, pad, body_size)
            .into_iter()
            .map(|mut p| {
                p.heading = None;
                p
            })
            .collect();
        let vmerge = if gc.row_span > 1 { VMerge::Restart } else { VMerge::None };
        // 網底：完整涵蓋儲存格的最小填色矩形
        let shading = fills
            .iter()
            .filter(|(f, _)| f.x0 <= rect.x0 + 2.0 && f.y0 <= rect.y0 + 2.0 && f.x1 >= rect.x1 - 2.0 && f.y1 >= rect.y1 - 2.0)
            .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
            .map(|f| f.1);
        // 文字明顯離開頂端且大致置中 → 垂直置中
        let valign_center = match lines.iter().map(|l| l.bbox).reduce(|a, b| a.union(&b)) {
            Some(tb) => tb.y0 - rect.y0 > 4.0 && (tb.cy() - rect.cy()).abs() < rect.height() * 0.15,
            None => false,
        };
        placed[gc.row].push((gc.col, Cell { col_span: gc.col_span, vmerge, shading, valign_center, paragraphs }));
        for r in gc.row + 1..gc.row + gc.row_span {
            let cont = Cell { col_span: gc.col_span, vmerge: VMerge::Continue, shading, valign_center, paragraphs: Vec::new() };
            placed[r].push((gc.col, cont));
        }
    }
    for (r, mut cells) in placed.into_iter().enumerate() {
        cells.sort_by_key(|c| c.0);
        rows[r].cells = cells.into_iter().map(|c| c.1).collect();
    }
    Table {
        bbox: grid.bbox,
        col_widths: grid.xs.windows(2).map(|w| w[1] - w[0]).collect(),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::lines::tests::glyphs_at;
    use super::*;
    use crate::model::Ruling;

    #[test]
    fn running_header_detected_and_removed() {
        let pages: Vec<PageData> = (1..=3)
            .map(|n| {
                let mut p = PageData { width: 600.0, height: 800.0, ..Default::default() };
                p.glyphs.extend(glyphs_at("Annual Report 2026", 50.0, 30.0, 9.0));
                p.glyphs.extend(glyphs_at(&format!("Page {n} of 3"), 50.0, 780.0, 9.0));
                p.glyphs.extend(glyphs_at(&format!("Body text {n}."), 50.0, 300.0, 10.0));
                p
            })
            .collect();
        let running = running_lines(&pages);
        assert!(running.contains("AnnualReport####"));
        let l = analyze_with(&pages[1], 10.0, &running);
        assert_eq!(l.blocks.len(), 1);
    }

    #[test]
    fn paragraph_continues_on_next_page() {
        let full = "x".repeat(80);
        let mut a = PageData { width: 600.0, height: 800.0, ..Default::default() };
        a.glyphs.extend(glyphs_at(&full, 50.0, 700.0, 10.0));
        a.glyphs.extend(glyphs_at(&full, 50.0, 712.0, 10.0));
        let mut b = PageData { width: 600.0, height: 800.0, ..Default::default() };
        b.glyphs.extend(glyphs_at("end of sentence.", 50.0, 100.0, 10.0));
        b.glyphs.extend(glyphs_at("New paragraph.", 50.0, 140.0, 10.0));
        let mut layouts = vec![analyze(&a, 10.0), analyze(&b, 10.0)];
        merge_across_pages(&mut layouts);
        let Block::Paragraph(p) = &layouts[0].blocks[0] else { panic!() };
        assert!(p.text().ends_with("x end of sentence."));
        assert_eq!(layouts[1].blocks.len(), 1);
    }

    #[test]
    fn page_number_patterns() {
        for t in ["12", "- 3 -", "4 / 20", "第 5 頁", "第5頁共9頁", "Page 2 of 9", "page 7"] {
            assert!(is_page_number(t), "{t}");
        }
        for t in ["第一章", "12 apples", "Pages", "12345"] {
            assert!(!is_page_number(t), "{t}");
        }
    }

    #[test]
    fn page_with_text_and_table() {
        let mut page = PageData { width: 600.0, height: 800.0, ..Default::default() };
        page.glyphs.extend(glyphs_at("Report", 260.0, 60.0, 20.0));
        let body = "x".repeat(80);
        page.glyphs.extend(glyphs_at(&body, 50.0, 100.0, 10.0));
        // 2x2 表格 y 150..190
        for y in [150.0, 170.0, 190.0] {
            page.rulings.push(Ruling::horizontal(y, 50.0, 450.0));
        }
        for x in [50.0, 250.0, 450.0] {
            page.rulings.push(Ruling::vertical(x, 150.0, 190.0));
        }
        page.glyphs.extend(glyphs_at("A1", 55.0, 164.0, 10.0));
        page.glyphs.extend(glyphs_at("B1", 255.0, 164.0, 10.0));
        page.glyphs.extend(glyphs_at("A2", 55.0, 184.0, 10.0));
        page.glyphs.extend(glyphs_at("After", 50.0, 220.0, 10.0));

        let layout = analyze(&page, body_size([&page]));
        let kinds: Vec<&str> = layout
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(_) => "p",
                Block::Table(_) => "t",
                Block::Image(_) => "i",
            })
            .collect();
        assert_eq!(kinds, vec!["p", "p", "t", "p"]);
        let Block::Paragraph(title) = &layout.blocks[0] else { panic!() };
        assert_eq!(title.heading, Some(1));
        let Block::Table(t) = &layout.blocks[2] else { panic!() };
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells[1].paragraphs[0].text(), "B1");
        assert!(t.rows[1].cells[1].paragraphs.is_empty());
    }
}
