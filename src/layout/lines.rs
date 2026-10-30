//! 把字元依基線聚合成行，並在大間距處切成「行片段」。

use crate::model::{Glyph, Rect, Run};

/// 一個行片段：同一基線、間距正常的一串字元
#[derive(Debug, Clone)]
pub struct Line {
    pub bbox: Rect,
    pub baseline: f32,
    pub size: f32,
    pub runs: Vec<Run>,
}

impl Line {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| if r.tab { "\t" } else { r.text.as_str() }).collect()
    }
    pub fn first_char(&self) -> Option<char> {
        self.runs.iter().find_map(|r| r.text.chars().next())
    }
    pub fn last_char(&self) -> Option<char> {
        self.runs.iter().rev().find_map(|r| r.text.chars().last())
    }
}

/// 中日韓字元（不以空白分詞）
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3000..=0x30FF | 0x3100..=0x312F | 0x31A0..=0x31FF |
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF |
        0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}

/// 切成不同片段的最小間距（以字級倍數計）
const SEGMENT_GAP: f32 = 1.8;

pub fn build_lines(glyphs: &[&Glyph]) -> Vec<Line> {
    let mut gs: Vec<&Glyph> = glyphs
        .iter()
        .copied()
        .filter(|g| !g.ch.is_whitespace() && !g.ch.is_control() && g.size > 0.5)
        .collect();
    if gs.is_empty() {
        return Vec::new();
    }
    gs.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));

    // 依基線分群
    let mut rows: Vec<Vec<&Glyph>> = Vec::new();
    let mut row_base = f32::NAN;
    let mut row_size = 0.0f32;
    for g in gs {
        let tol = 0.35 * row_size.max(g.size);
        if rows.is_empty() || (g.baseline - row_base).abs() > tol {
            rows.push(vec![g]);
            row_base = g.baseline;
            row_size = g.size;
        } else {
            rows.last_mut().unwrap().push(g);
            row_size = row_size.max(g.size);
        }
    }

    let mut out = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
        let mut start = 0;
        for i in 1..=row.len() {
            let split = i == row.len() || {
                let prev = row[i - 1];
                let cur = row[i];
                let size = prev.size.max(cur.size);
                cur.bbox.x0 - prev.bbox.x1 > SEGMENT_GAP * size
            };
            if split {
                out.push(make_line(&row[start..i]));
                start = i;
            }
        }
    }
    out
}

fn same_style(r: &Run, g: &Glyph) -> bool {
    r.font == g.font
        && (r.size - g.size).abs() < 0.6
        && r.bold == g.bold
        && r.italic == g.italic
        && r.color == g.color
}

fn make_line(gs: &[&Glyph]) -> Line {
    let mut bbox = gs[0].bbox;
    let mut runs: Vec<Run> = Vec::new();
    let mut size_sum = 0.0;
    let mut prev: Option<&Glyph> = None;
    for &g in gs {
        bbox = bbox.union(&g.bbox);
        size_sum += g.size;
        // 依間距補空白；中日韓文字之間只有很大的間距才補
        let mut space = false;
        if let Some(p) = prev {
            let gap = g.bbox.x0 - p.bbox.x1;
            let size = p.size.max(g.size);
            // 項目符號後一律依一般間距補空白
            let cjk = (is_cjk(p.ch) || is_cjk(g.ch)) && p.ch != '•';
            space = g.space_before || if cjk { gap > 1.0 * size } else { gap > 0.15 * size };
        }
        match runs.last_mut() {
            Some(r) if same_style(r, g) => {
                if space {
                    r.text.push(' ');
                }
                r.text.push(g.ch);
            }
            _ => {
                if space {
                    if let Some(r) = runs.last_mut() {
                        r.text.push(' ');
                    }
                }
                runs.push(Run {
                    text: g.ch.to_string(),
                    font: g.font.clone(),
                    size: g.size,
                    bold: g.bold,
                    italic: g.italic,
                    color: g.color,
                    tab: false,
                });
            }
        }
        prev = Some(g);
    }
    let baseline = gs.iter().map(|g| g.baseline).sum::<f32>() / gs.len() as f32;
    Line { bbox, baseline, size: size_sum / gs.len() as f32, runs }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 測試用：在 (x, baseline) 依序排出字元，每字寬 0.5*size（CJK 為 1*size）
    pub fn glyphs_at(text: &str, x: f32, baseline: f32, size: f32) -> Vec<Glyph> {
        let mut out = Vec::new();
        let mut cx = x;
        let mut space = false;
        for ch in text.chars() {
            let w = if is_cjk(ch) { size } else { size * 0.5 };
            if ch != ' ' {
                out.push(Glyph {
                    ch,
                    bbox: Rect::new(cx, baseline - size * 0.8, cx + w, baseline + size * 0.2),
                    baseline,
                    size,
                    font: "Test".into(),
                    bold: false,
                    italic: false,
                    color: [0, 0, 0],
                    space_before: std::mem::take(&mut space),
                });
            } else {
                space = true;
            }
            cx += if ch == ' ' { size * 0.3 } else { w };
        }
        out
    }

    #[test]
    fn words_get_spaces() {
        let g = glyphs_at("Hello world", 10.0, 100.0, 10.0);
        let refs: Vec<&Glyph> = g.iter().collect();
        let lines = build_lines(&refs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Hello world");
    }

    #[test]
    fn cjk_has_no_spaces() {
        let g = glyphs_at("繁體中文測試", 10.0, 100.0, 12.0);
        let refs: Vec<&Glyph> = g.iter().collect();
        assert_eq!(build_lines(&refs)[0].text(), "繁體中文測試");
    }

    #[test]
    fn explicit_space_between_cjk_kept() {
        let g = glyphs_at("一月 二月", 10.0, 100.0, 10.0);
        let refs: Vec<&Glyph> = g.iter().collect();
        assert_eq!(build_lines(&refs)[0].text(), "一月 二月");
    }

    #[test]
    fn two_rows_and_big_gap_split() {
        let mut g = glyphs_at("Left", 10.0, 100.0, 10.0);
        g.extend(glyphs_at("Right", 300.0, 100.5, 10.0));
        g.extend(glyphs_at("Next", 10.0, 114.0, 10.0));
        let refs: Vec<&Glyph> = g.iter().collect();
        let lines = build_lines(&refs);
        let texts: Vec<String> = lines.iter().map(|l| l.text()).collect();
        assert_eq!(texts, vec!["Left", "Right", "Next"]);
    }

    #[test]
    fn style_change_makes_new_run() {
        let mut g = glyphs_at("ab", 10.0, 100.0, 10.0);
        let mut b = glyphs_at("cd", 20.0, 100.0, 10.0);
        for x in &mut b {
            x.bold = true;
        }
        g.extend(b);
        let refs: Vec<&Glyph> = g.iter().collect();
        let lines = build_lines(&refs);
        assert_eq!(lines[0].runs.len(), 2);
        assert!(lines[0].runs[1].bold);
    }
}
