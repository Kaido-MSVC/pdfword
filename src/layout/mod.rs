//! 版面分析：PageData（純資料）→ PageLayout（段落、表格、圖片的閱讀順序）。

pub mod blocks;
pub mod columns;
pub mod figures;
pub mod lines;
pub mod tables;

use crate::model::{Block, Cell, Glyph, PageData, PageLayout, Rect, Row, Table, VMerge};
use columns::{Item, ItemKind};
use lines::Line;
use std::collections::HashMap;

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

pub fn analyze(page: &PageData, body_size: f32) -> PageLayout {
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

    let mut contents: Vec<Content> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    for l in lines::build_lines(&free) {
        items.push(Item { bbox: l.bbox, kind: ItemKind::Text });
        contents.push(Content::Line(l));
    }
    for (grid, glyphs) in grids.iter().zip(in_table) {
        items.push(Item { bbox: grid.bbox, kind: ItemKind::Other });
        contents.push(Content::Table(build_table(grid, &glyphs, body_size)));
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
            first.space_before = (top - b - 0.25 * first.size).clamp(0.0, 72.0);
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

fn build_table(grid: &tables::TableGrid, glyphs: &[&Glyph], body_size: f32) -> Table {
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
        placed[gc.row].push((gc.col, Cell { col_span: gc.col_span, vmerge, paragraphs }));
        for r in gc.row + 1..gc.row + gc.row_span {
            placed[r].push((gc.col, Cell { col_span: gc.col_span, vmerge: VMerge::Continue, paragraphs: Vec::new() }));
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
