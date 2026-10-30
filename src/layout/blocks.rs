//! 把同一欄（或同一儲存格）的行片段合併成段落，判斷對齊、縮排與標題。

use super::lines::{is_cjk, Line};
use crate::model::{Align, Paragraph, Rect, Run};

fn is_bullet_start(l: &Line) -> bool {
    let t = l.text();
    let mut cs = t.chars();
    match cs.next() {
        Some('•' | '·' | '●' | '○' | '◦' | '▪' | '■' | '□' | '◆' | '►' | '➢' | '✓' | '※') => true,
        Some(c) if c.is_ascii_digit() => {
            let rest: String = t.chars().skip_while(|c| c.is_ascii_digit()).take(1).collect();
            matches!(rest.as_str(), "." | ")" | "、")
        }
        Some('(' | '（') => true,
        Some(c) => "一二三四五六七八九十".contains(c) && t.chars().nth(1) == Some('、'),
        None => false,
    }
}

/// 同一基線、彼此分開的片段（例如「標籤 … 數值」）以 Tab 合為一行
fn join_same_baseline(lines: &[Line]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for l in lines {
        if let Some(p) = out.last_mut() {
            if (p.baseline - l.baseline).abs() < 0.3 * p.size.max(l.size) && l.bbox.x0 > p.bbox.x1 {
                let mut tab = p.runs.last().cloned().unwrap();
                tab.text.clear();
                tab.tab = true;
                p.runs.push(tab);
                p.runs.extend(l.runs.iter().cloned());
                p.bbox = p.bbox.union(&l.bbox);
                continue;
            }
        }
        out.push(l.clone());
    }
    out
}

fn same_style(a: &Run, b: &Run) -> bool {
    !a.tab && !b.tab && a.font == b.font && (a.size - b.size).abs() < 0.6 && a.bold == b.bold
        && a.italic == b.italic && a.underline == b.underline && a.color == b.color
}

fn push_runs(dst: &mut Vec<Run>, src: &[Run]) {
    for r in src {
        match dst.last_mut() {
            Some(last) if same_style(last, r) => last.text.push_str(&r.text),
            _ => dst.push(r.clone()),
        }
    }
}

/// 把下一行接到段落：處理連字號斷字與中日韓不加空白
fn append_line(runs: &mut Vec<Run>, line: &Line) {
    append_runs(runs, &line.runs);
}

/// 把下一行（或下一頁開頭）的文字接到段落後面
pub fn append_runs(runs: &mut Vec<Run>, next: &[Run]) {
    let prev_last = runs.iter().rev().find_map(|r| r.text.chars().last());
    let next_first = next.iter().find_map(|r| r.text.chars().next());
    if let (Some(p), Some(n)) = (prev_last, next_first) {
        if p == '-' && n.is_lowercase() {
            if let Some(r) = runs.iter_mut().rev().find(|r| !r.text.is_empty()) {
                r.text.pop();
            }
        } else if !(is_cjk(p) || is_cjk(n)) {
            if let Some(r) = runs.iter_mut().rev().find(|r| !r.tab) {
                r.text.push(' ');
            }
        }
    }
    push_runs(runs, next);
}

pub fn paragraphs(lines: &[Line], col: Rect, body_size: f32) -> Vec<Paragraph> {
    let lines = join_same_baseline(lines);
    if lines.is_empty() {
        return Vec::new();
    }
    // 依段落切分
    let mut groups: Vec<Vec<&Line>> = Vec::new();
    for l in &lines {
        let new_para = match groups.last().and_then(|g| g.last()) {
            None => true,
            Some(p) => {
                let size = p.size.max(l.size);
                let gap = l.baseline - p.baseline;
                // 兩行都置中於欄（而不只是彼此中心相近）
                let near_center = |r: &Rect| (r.cx() - col.cx()).abs() < 0.04 * col.width() + 3.0;
                let centered = (p.bbox.cx() - l.bbox.cx()).abs() < 3.0
                    && near_center(&p.bbox)
                    && near_center(&l.bbox)
                    && p.bbox.x0 > col.x0 + 2.0 * size;
                let prev_short = p.bbox.x1 < col.x1 - 2.5 * size;
                // 比上一行更縮排 → 新段落的首行縮排
                let indented = l.bbox.x0 > p.bbox.x0 + 1.2 * size && !centered;
                (l.size - p.size).abs() > 0.2 * p.size
                    || gap > 1.75 * size
                    || gap < 0.0
                    || (prev_short && !centered)
                    || indented
                    || is_bullet_start(l)
                    || l.runs.iter().any(|r| r.tab) != p.runs.iter().any(|r| r.tab)
            }
        };
        if new_para {
            groups.push(vec![l]);
        } else {
            groups.last_mut().unwrap().push(l);
        }
    }

    let mut out = Vec::new();
    let mut prev_bottom: Option<f32> = None;
    for g in groups {
        let first = g[0];
        let size = g.iter().map(|l| l.size).sum::<f32>() / g.len() as f32;
        let left = if g.len() > 1 {
            g[1..].iter().map(|l| l.bbox.x0).fold(f32::MAX, f32::min)
        } else {
            first.bbox.x0
        };
        let width = col.width().max(1.0);
        // 單行中文且左緣剛好空出 1–4 個字寬：是首行縮排，不是置中
        let indent_chars = (first.bbox.x0 - col.x0) / size;
        let cjk_indent = g.len() == 1
            && first.first_char().is_some_and(is_cjk)
            && (1.0..=4.0).contains(&indent_chars.round())
            && (indent_chars - indent_chars.round()).abs() < 0.12;
        let all_centered = g.iter().all(|l| (l.bbox.cx() - col.cx()).abs() < 0.04 * width + 2.0)
            && first.bbox.x0 > col.x0 + 2.0 * size
            && !cjk_indent;
        let right_aligned = g.iter().all(|l| (col.x1 - l.bbox.x1).abs() < 3.0) && left > col.x0 + 0.3 * width;
        let justified = g.len() >= 2 && g[..g.len() - 1].iter().all(|l| col.x1 - l.bbox.x1 < 1.0 * size);
        let align = if all_centered {
            Align::Center
        } else if right_aligned {
            Align::Right
        } else if justified {
            Align::Justify
        } else {
            Align::Left
        };

        let mut runs = Vec::new();
        push_runs(&mut runs, &first.runs);
        for l in &g[1..] {
            append_line(&mut runs, l);
        }

        let (indent_left, indent_first) = match align {
            Align::Center | Align::Right => (0.0, 0.0),
            _ => {
                let il = (left - col.x0).max(0.0);
                (il, first.bbox.x0 - col.x0 - il)
            }
        };
        let space_before = match prev_bottom {
            Some(b) => (first.bbox.y0 - b - 0.25 * size).clamp(0.0, 72.0),
            None => 0.0,
        };
        prev_bottom = Some(g.last().unwrap().bbox.y1);

        let text_len: usize = runs.iter().map(|r| r.text.chars().count()).sum();
        let all_bold = runs.iter().filter(|r| !r.tab).all(|r| r.bold);
        let ratio = if body_size > 0.0 { size / body_size } else { 1.0 };
        let heading = if text_len > 150 || g.len() > 3 {
            None
        } else if ratio >= 1.6 {
            Some(1)
        } else if ratio >= 1.3 {
            Some(2)
        } else if ratio >= 1.12 && all_bold {
            Some(3)
        } else {
            None
        };

        out.push(Paragraph { runs, align, heading, indent_left, indent_first, space_before, size });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::lines::{build_lines, tests::glyphs_at};
    use super::*;
    use crate::model::Glyph;

    fn lines_of(rows: &[(&str, f32, f32)], size: f32) -> Vec<Line> {
        let mut g: Vec<Glyph> = Vec::new();
        for &(t, x, y) in rows {
            g.extend(glyphs_at(t, x, y, size));
        }
        let refs: Vec<&Glyph> = g.iter().collect();
        build_lines(&refs)
    }

    #[test]
    fn wrapped_lines_join_into_one_paragraph() {
        // 欄寬 50..250；字寬 5pt。前兩行填滿欄寬，第三行較短
        let full = "x".repeat(40);
        let rows = [(full.as_str(), 50.0, 100.0), (full.as_str(), 50.0, 112.0), ("end", 50.0, 124.0)];
        let lines = lines_of(&rows, 10.0);
        let col = Rect::new(50.0, 90.0, 250.0, 130.0);
        let p = paragraphs(&lines, col, 10.0);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].align, Align::Justify);
        assert!(p[0].text().ends_with(" end"));
    }

    #[test]
    fn short_line_and_big_gap_split_paragraphs() {
        let full = "x".repeat(40);
        let rows = [(full.as_str(), 50.0, 100.0), ("short", 50.0, 112.0), (full.as_str(), 50.0, 140.0)];
        let lines = lines_of(&rows, 10.0);
        let p = paragraphs(&lines, Rect::new(50.0, 90.0, 250.0, 150.0), 10.0);
        assert_eq!(p.len(), 2);
        assert!(p[1].space_before > 5.0);
    }

    #[test]
    fn hyphenation_and_cjk_join() {
        let a = "x".repeat(36) + "inter-";
        let rows = [(a.as_str(), 50.0, 100.0), ("national", 50.0, 112.0)];
        let p = paragraphs(&lines_of(&rows, 10.0), Rect::new(50.0, 90.0, 250.0, 130.0), 10.0);
        assert!(p[0].text().ends_with("international"));

        let c = "中".repeat(20);
        let rows = [(c.as_str(), 50.0, 100.0), ("文字", 50.0, 114.0)];
        let p = paragraphs(&lines_of(&rows, 10.0), Rect::new(50.0, 90.0, 250.0, 130.0), 10.0);
        assert!(p[0].text().ends_with("中文字"));
    }

    #[test]
    fn centered_heading_detected() {
        let rows = [("Title", 125.0, 100.0)];
        let p = paragraphs(&lines_of(&rows, 20.0), Rect::new(50.0, 80.0, 250.0, 130.0), 10.0);
        assert_eq!(p[0].align, Align::Center);
        assert_eq!(p[0].heading, Some(1));
    }

    #[test]
    fn short_left_aligned_lines_with_similar_centers_split() {
        let rows = [("abcdefghij", 93.0, 100.0), ("abcdefghik", 93.0, 112.0)];
        let p = paragraphs(&lines_of(&rows, 10.0), Rect::new(50.0, 90.0, 500.0, 130.0), 10.0);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn bullets_start_new_paragraphs() {
        let rows = [("• one", 50.0, 100.0), ("• two", 50.0, 112.0)];
        let p = paragraphs(&lines_of(&rows, 10.0), Rect::new(50.0, 90.0, 250.0, 130.0), 10.0);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn label_value_uses_tab() {
        let rows = [("Name", 50.0, 100.0), ("Value", 200.0, 100.0)];
        let p = paragraphs(&lines_of(&rows, 10.0), Rect::new(50.0, 90.0, 250.0, 130.0), 10.0);
        assert_eq!(p[0].text(), "Name\tValue");
    }
}
