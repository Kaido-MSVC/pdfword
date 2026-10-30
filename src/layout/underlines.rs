//! 表格外緊貼在文字基線下方的水平線視為底線；底線下沒有文字的部分（表單的填空欄）
//! 補上加底線的全形空白，讓 Word 中仍保留可填寫的空格長度。

use crate::model::{Glyph, Rect, Ruling};

/// 底線與基線的最大距離（以字級倍數計）
const MAX_DROP: f32 = 0.4;

/// `rulings` 應只含表格以外的線。回傳處理後的字元（含補上的空白）。
pub fn apply(glyphs: &[&Glyph], rulings: &[Ruling]) -> Vec<Glyph> {
    let mut out: Vec<Glyph> = glyphs.iter().map(|&g| g.clone()).collect();
    let n = out.len();
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
        on.sort_by(|&a, &b| out[a].bbox.x0.total_cmp(&out[b].bbox.x0));
        let proto = out[reference].clone();
        let mut cursor = r.x0;
        let mut fills = Vec::new();
        for &i in &on {
            out[i].underline = true;
            fill(&proto, cursor, out[i].bbox.x0, &mut fills);
            cursor = cursor.max(out[i].bbox.x1);
        }
        fill(&proto, cursor, r.x1, &mut fills);
        out.extend(fills);
    }
    out
}

fn dist(b: &Rect, r: &Ruling) -> f32 {
    (r.x0 - b.x1).max(b.x0 - r.x1).max(0.0)
}

/// 在 [a, b] 間補上與字級等寬的加底線全形空白
fn fill(proto: &Glyph, a: f32, b: f32, out: &mut Vec<Glyph>) {
    let count = ((b - a) / proto.size).round();
    if count < 1.0 {
        return;
    }
    let w = (b - a) / count;
    for k in 0..count as usize {
        let x = a + k as f32 * w;
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
    fn blank_between_words_becomes_underlined_spaces() {
        // 「姓名」(10..30) + 空白欄 (30..70) + 「先生」(70..90)，底線在 30..70
        let mut g = glyphs_at("姓名", 10.0, 100.0, 10.0);
        g.extend(glyphs_at("先生", 70.0, 100.0, 10.0));
        let refs: Vec<&Glyph> = g.iter().collect();
        let out = apply(&refs, &[Ruling::horizontal(102.0, 30.0, 70.0)]);
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
}
