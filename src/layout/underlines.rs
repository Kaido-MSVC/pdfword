//! 表格外緊貼在文字基線下方的水平線視為底線；底線下沒有文字的部分（表單的填空欄）
//! 補上加底線的全形空白，讓 Word 中仍保留可填寫的空格長度。

use crate::model::{Glyph, Rect, Ruling};

/// 底線與基線的最大距離（以字級倍數計）
const MAX_DROP: f32 = 0.4;
/// 每條底線的補字上限，包含線上的所有空白區間。
const MAX_FILL_GLYPHS: u16 = 4096;
/// 單頁可容納八條達到上限的底線，保留密集表單的空白欄並限制額外配置。
const MAX_PAGE_FILL_GLYPHS: u16 = MAX_FILL_GLYPHS * 8;

/// `rulings` 應只含表格以外的線。回傳處理後的字元（含補上的空白）。
pub fn apply(glyphs: &[&Glyph], rulings: &[Ruling]) -> Vec<Glyph> {
    let mut out: Vec<Glyph> = glyphs.iter().map(|&g| g.clone()).collect();
    let n = out.len();
    let mut page_remaining = MAX_PAGE_FILL_GLYPHS;
    for r in rulings.iter().filter(|r| r.is_horizontal()) {
        let y = r.y0;
        let above = |g: &Glyph| {
            let d = y - g.baseline;
            !g.ch.is_whitespace() && d >= -0.05 * g.size && d <= MAX_DROP * g.size
        };
        // 同一行、位於線上或緊鄰線兩端的字元，作為字級與字型的參考
        let Some(reference) = (0..n)
            .filter(|&i| above(&out[i]))
            .filter(|&i| {
                let g = &out[i];
                g.bbox.x1 >= r.x0 - 2.0 * g.size && g.bbox.x0 <= r.x1 + 2.0 * g.size
            })
            .min_by(|&a, &b| dist(&out[a].bbox, r).total_cmp(&dist(&out[b].bbox, r)))
        else {
            continue;
        };
        let mut on: Vec<usize> = (0..n)
            .filter(|&i| above(&out[i]) && (r.x0..=r.x1).contains(&out[i].bbox.cx()))
            .collect();
        for &i in &on {
            out[i].underline = true;
        }
        if page_remaining == 0 {
            continue;
        }

        on.sort_by(|&a, &b| out[a].bbox.x0.total_cmp(&out[b].bbox.x0));
        let proto = &out[reference];
        let mut cursor = r.x0;
        let mut fills = Vec::new();
        let line_budget = page_remaining.min(MAX_FILL_GLYPHS);
        let mut line_remaining = line_budget;
        for i in on {
            fill(
                proto,
                cursor,
                out[i].bbox.x0,
                &mut line_remaining,
                &mut fills,
            );
            cursor = cursor.max(out[i].bbox.x1);
        }
        fill(proto, cursor, r.x1, &mut line_remaining, &mut fills);
        page_remaining -= line_budget - line_remaining;
        out.extend(fills);
    }
    out
}

fn dist(b: &Rect, r: &Ruling) -> f32 {
    (r.x0 - b.x1).max(b.x0 - r.x1).max(0.0)
}

/// 補上整個空白區間。若剩餘預算不足，略過這個區間。
fn fill(proto: &Glyph, a: f32, b: f32, remaining: &mut u16, out: &mut Vec<Glyph>) {
    if *remaining == 0 || !proto.size.is_finite() || proto.size <= 0.0 {
        return;
    }
    let count = ((b - a) / proto.size).round();
    if !(1.0..=f32::from(*remaining)).contains(&count) {
        return;
    }
    let w = (b - a) / count;
    let count = count as u16;
    *remaining -= count;
    for k in 0..count {
        let x = a + f32::from(k) * w;
        out.push(Glyph {
            ch: '\u{3000}',
            bbox: Rect::new(x, proto.bbox.y0, x + w, proto.bbox.y1),
            bold: false,
            italic: false,
            space_before: false,
            underline: true,
            ..proto.clone()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::lines::{build_lines, tests::glyphs_at};
    use super::*;

    #[test]
    fn invalid_blank_width_or_font_size_is_ignored() {
        let mut glyphs = glyphs_at("字", 10.0, 100.0, 10.0);
        let glyph = &mut glyphs[0];
        let mut out = Vec::new();
        let mut remaining = MAX_FILL_GLYPHS;
        fill(glyph, 10.0, 10.0, &mut remaining, &mut out);
        fill(glyph, 10.0, f32::INFINITY, &mut remaining, &mut out);
        glyph.size = 0.0;
        fill(glyph, 10.0, 20.0, &mut remaining, &mut out);
        assert!(out.is_empty());
        assert_eq!(remaining, MAX_FILL_GLYPHS);
    }

    #[test]
    fn blank_between_words_becomes_underlined_spaces() {
        // 「姓名」(10..30) + 空白欄 (30..70) + 「先生」(70..90)，底線在 30..70
        let mut g = glyphs_at("姓名", 10.0, 100.0, 10.0);
        g.extend(glyphs_at("先生", 70.0, 100.0, 10.0));
        let refs: Vec<&Glyph> = g.iter().collect();
        let out = apply(&refs, &[Ruling::horizontal(102.0, 30.0, 70.0)]);
        assert_eq!(out[4].bbox, Rect::new(30.0, 92.0, 40.0, 102.0));
        assert_eq!(out[7].bbox, Rect::new(60.0, 92.0, 70.0, 102.0));
        let refs: Vec<&Glyph> = out.iter().collect();
        let lines = build_lines(&refs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "姓名\u{3000}\u{3000}\u{3000}\u{3000}先生");
        let u: Vec<bool> = lines[0].runs.iter().map(|r| r.underline).collect();
        assert_eq!(u, vec![false, true, false]);
    }

    #[test]
    fn underlined_text_and_unrelated_lines() {
        let g = glyphs_at("重要", 10.0, 100.0, 10.0);
        let refs: Vec<&Glyph> = g.iter().collect();
        // 文字正下方的底線；遠在下方的分隔線不影響
        let out = apply(&refs, &[Ruling::horizontal(101.5, 10.0, 30.0), Ruling::horizontal(130.0, 0.0, 500.0)]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|g| g.underline));
    }

    #[test]
    fn oversized_gap_does_not_spend_budget_or_create_glyphs() {
        let glyphs = glyphs_at("字", 0.0, 100.0, 10.0);
        let mut remaining = MAX_FILL_GLYPHS;
        let mut fills = Vec::new();
        let end = f32::from(MAX_FILL_GLYPHS + 1) * glyphs[0].size;
        fill(&glyphs[0], 0.0, end, &mut remaining, &mut fills);
        assert_eq!(fills, Vec::<Glyph>::new());
        assert_eq!(remaining, MAX_FILL_GLYPHS);
    }

    #[test]
    fn gaps_on_the_same_ruling_share_the_line_budget() {
        let size = 10.0;
        let left_count = MAX_FILL_GLYPHS / 2;
        let x = size * f32::from(left_count);
        let glyphs = glyphs_at("字", x, 100.0, size);
        let end = glyphs[0].bbox.x1 + size * f32::from(left_count + 1);
        let out = apply(&[&glyphs[0]], &[Ruling::horizontal(102.0, 0.0, end)]);
        assert_eq!(out.len(), 1 + usize::from(left_count));
        assert!(out[0].underline);
        assert!(out[1..].iter().all(|g| g.bbox.x1 <= x));
    }

    #[test]
    fn rulings_share_page_budget_and_still_mark_original_text() {
        let size = 10.0;
        let line_count = MAX_PAGE_FILL_GLYPHS / MAX_FILL_GLYPHS + 1;
        let glyphs: Vec<_> = (0..line_count)
            .flat_map(|row| glyphs_at("字", 0.0, 20.0 * f32::from(row), size))
            .collect();
        let refs: Vec<_> = glyphs.iter().collect();
        let rulings: Vec<_> = (0..line_count)
            .map(|row| {
                let glyph = &glyphs[usize::from(row)];
                // 前七條用滿各自額度，第八條保留一個字元的單頁預算。
                let count = MAX_FILL_GLYPHS - u16::from(row + 2 == line_count);
                let end = glyph.bbox.x1 + size * f32::from(count);
                Ruling::horizontal(glyph.baseline + 2.0, 0.0, end)
            })
            .collect();
        let out = apply(&refs, &rulings);
        // 最後一條線的空白超出剩餘預算，原有字元仍加上底線。
        assert_eq!(
            out.len() - refs.len(),
            usize::from(MAX_PAGE_FILL_GLYPHS - 1)
        );
        assert!(out[..refs.len()].iter().all(|g| g.underline));

        let mut exact_budget = rulings;
        let last = usize::from(line_count - 1);
        exact_budget[last - 1].x1 += size;
        let out = apply(&refs, &exact_budget);
        assert_eq!(out.len() - refs.len(), usize::from(MAX_PAGE_FILL_GLYPHS));
        assert!(out[last].underline);
    }
}
