//! PageLayout → DOCX（docx-rs）。
//!
//! 採流式輸出：頁與頁之間不插入硬分頁，由 Word 重新分頁；頁面尺寸改變時才另開 Section。

use crate::fonts::{map_font, DEFAULT_EAST_ASIA, DEFAULT_LATIN};
use crate::model::{self, Align, Block, PageLayout, Rect, VMerge};
use docx_rs::*;
use std::io::{Seek, Write};

/// pt → twip
fn tw(pt: f32) -> i32 {
    (pt * 20.0).round() as i32
}

/// pt → EMU
fn emu(pt: f32) -> u32 {
    (pt.max(0.0) * 12700.0).round() as u32
}

enum Child {
    P(Paragraph),
    T(Table),
}

struct SectionSpec {
    width: f32,
    height: f32,
    margin: (f32, f32, f32, f32), // 上、右、下、左
    children: Vec<Child>,
}

pub fn write_docx<W: Write + Seek>(pages: &[PageLayout], body_size: f32, out: W) -> Result<(), String> {
    let mut sections: Vec<SectionSpec> = Vec::new();
    // 依頁面尺寸分組
    let mut i = 0;
    while i < pages.len() {
        let (w, h) = (pages[i].width, pages[i].height);
        let mut j = i + 1;
        while j < pages.len() && (pages[j].width - w).abs() < 2.0 && (pages[j].height - h).abs() < 2.0 {
            j += 1;
        }
        sections.push(build_section(&pages[i..j]));
        i = j;
    }

    let mut docx = Docx::new()
        .default_fonts(
            RunFonts::new()
                .ascii(DEFAULT_LATIN)
                .hi_ansi(DEFAULT_LATIN)
                .east_asia(DEFAULT_EAST_ASIA)
                .cs(DEFAULT_LATIN),
        )
        .default_size(((body_size * 2.0).round() as usize).clamp(8, 144))
        .default_line_spacing(LineSpacing::new().before(0).after(0));
    for (lvl, name) in [(1, "heading 1"), (2, "heading 2"), (3, "heading 3")] {
        docx = docx.add_style(
            Style::new(format!("Heading{lvl}"), StyleType::Paragraph)
                .name(name)
                .based_on("Normal")
                .outline_lvl(lvl - 1),
        );
    }

    let last = sections.len().saturating_sub(1);
    for (k, s) in sections.into_iter().enumerate() {
        let size = PageSize::new().size(tw(s.width) as u32, tw(s.height) as u32);
        let size = if s.width > s.height {
            size.orient(PageOrientationType::Landscape)
        } else {
            size
        };
        let margin = PageMargin::new()
            .top(tw(s.margin.0))
            .right(tw(s.margin.1))
            .bottom(tw(s.margin.2))
            .left(tw(s.margin.3))
            .header(tw(s.margin.0 / 2.0))
            .footer(tw(s.margin.2 / 2.0));
        if k == last {
            // 最後一節使用文件本身的節屬性，避免多出空白頁
            docx = docx.page_size(tw(s.width) as u32, tw(s.height) as u32).page_margin(margin);
            if s.width > s.height {
                docx = docx.page_orient(PageOrientationType::Landscape);
            }
            if s.children.is_empty() {
                docx = docx.add_paragraph(Paragraph::new());
            }
            for c in s.children {
                docx = match c {
                    Child::P(p) => docx.add_paragraph(p),
                    Child::T(t) => docx.add_table(t),
                };
            }
        } else {
            let mut sec = Section::new().page_size(size).page_margin(margin);
            if s.children.is_empty() {
                sec = sec.add_paragraph(Paragraph::new());
            }
            for c in s.children {
                sec = match c {
                    Child::P(p) => sec.add_paragraph(p),
                    Child::T(t) => sec.add_table(t),
                };
            }
            docx = docx.add_section(sec);
        }
    }

    docx.build().pack(out).map_err(|e| format!("寫入 DOCX 失敗：{e}"))
}

fn build_section(pages: &[PageLayout]) -> SectionSpec {
    let (w, h) = (pages[0].width, pages[0].height);
    let content = pages
        .iter()
        .filter(|p| !p.blocks.is_empty())
        .map(|p| p.content)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Rect::new(72.0, 72.0, w - 72.0, h - 72.0));
    let clamp = |v: f32| v.clamp(18.0, 144.0);
    let margin = (clamp(content.y0), clamp(w - content.x1), clamp(h - content.y1), clamp(content.x0));
    let text_w = (w - margin.1 - margin.3).max(72.0);
    let text_h = (h - margin.0 - margin.2).max(72.0);

    let mut children = Vec::new();
    for page in pages {
        for b in &page.blocks {
            match b {
                Block::Paragraph(p) => {
                    if let Some(x) = paragraph(p, 0.0) {
                        children.push(Child::P(x));
                    }
                }
                Block::Table(t) => {
                    let offset = (t.bbox.x0 - margin.3).max(0.0);
                    children.push(Child::T(table(t, offset, text_w)));
                    // 相鄰表格會被 Word 合併，表格後補一個空段落
                    children.push(Child::P(Paragraph::new()));
                }
                Block::Image(img) => children.push(Child::P(image(img, page.content, margin.3, text_w, text_h))),
            }
        }
    }
    SectionSpec { width: w, height: h, margin, children }
}

fn run(r: &model::Run) -> Run {
    if r.tab {
        return Run::new().add_tab();
    }
    let (latin, east) = map_font(&r.font);
    let mut x = Run::new()
        .add_text(&r.text)
        .size(((r.size * 2.0).round() as usize).clamp(2, 3276))
        .fonts(RunFonts::new().ascii(&latin).hi_ansi(&latin).cs(&latin).east_asia(&east));
    if r.bold {
        x = x.bold();
    }
    if r.italic {
        x = x.italic();
    }
    if r.underline {
        x = x.underline("single");
    }
    if r.color != [0, 0, 0] {
        x = x.color(format!("{:02X}{:02X}{:02X}", r.color[0], r.color[1], r.color[2]));
    }
    x
}

fn paragraph(p: &model::Paragraph, extra_indent: f32) -> Option<Paragraph> {
    if p.runs.iter().all(|r| !r.tab && r.text.trim().is_empty()) {
        return None;
    }
    let mut x = Paragraph::new();
    for r in &p.runs {
        x = x.add_run(run(r));
    }
    x = x.align(match p.align {
        Align::Left => AlignmentType::Left,
        Align::Center => AlignmentType::Center,
        Align::Right => AlignmentType::Right,
        Align::Justify => AlignmentType::Both,
    });
    let left = tw(p.indent_left + extra_indent);
    let first = tw(p.indent_first);
    let special = match first {
        f if f > 0 => Some(SpecialIndentType::FirstLine(f)),
        f if f < 0 => Some(SpecialIndentType::Hanging(-f)),
        _ => None,
    };
    if left > 0 || special.is_some() {
        x = x.indent(Some(left.max(0)), special, None, None);
    }
    x = x.line_spacing(LineSpacing::new().before(tw(p.space_before).max(0) as u32).after(0));
    if let Some(h) = p.heading {
        x = x.style(&format!("Heading{h}")).outline_lvl(h as usize - 1);
    }
    Some(x)
}

fn table(t: &model::Table, offset: f32, text_w: f32) -> Table {
    let total: f32 = t.col_widths.iter().sum();
    // 超出版面寬度時等比例縮小
    let avail = (text_w - offset).max(text_w * 0.5);
    let scale = if total > avail { avail / total } else { 1.0 };
    let offset = if total * scale + offset > text_w { 0.0 } else { offset };
    let widths: Vec<usize> = t.col_widths.iter().map(|w| tw(w * scale).max(1) as usize).collect();

    let rows = t
        .rows
        .iter()
        .map(|r| {
            let mut col = 0;
            let cells = r
                .cells
                .iter()
                .map(|c| {
                    let w: usize = widths[col.min(widths.len() - 1)..(col + c.col_span).min(widths.len())].iter().sum();
                    col += c.col_span;
                    let mut cell = TableCell::new().width(w, WidthType::Dxa);
                    let paras: Vec<Paragraph> = c.paragraphs.iter().filter_map(|p| paragraph(p, 0.0)).collect();
                    if paras.is_empty() {
                        cell = cell.add_paragraph(Paragraph::new());
                    }
                    for p in paras {
                        cell = cell.add_paragraph(p);
                    }
                    if c.col_span > 1 {
                        cell = cell.grid_span(c.col_span);
                    }
                    if let Some([r, g, b]) = c.shading {
                        let fill = format!("{r:02X}{g:02X}{b:02X}");
                        cell = cell.shading(Shading::new().shd_type(ShdType::Clear).color("auto").fill(fill));
                    }
                    if c.valign_center {
                        cell = cell.vertical_align(VAlignType::Center);
                    }
                    match c.vmerge {
                        VMerge::Restart => cell.vertical_merge(VMergeType::Restart),
                        VMerge::Continue => cell.vertical_merge(VMergeType::Continue),
                        VMerge::None => cell,
                    }
                })
                .collect();
            TableRow::new(cells).row_height(tw(r.height) as f32).height_rule(HeightRule::AtLeast)
        })
        .collect();

    let mut table = Table::new(rows)
        .set_grid(widths.clone())
        .layout(TableLayoutType::Fixed)
        .width(widths.iter().sum(), WidthType::Dxa);
    if offset > 1.0 {
        table = table.indent(tw(offset));
    }
    table
}

fn image(img: &model::ImageItem, content: Rect, margin_left: f32, text_w: f32, text_h: f32) -> Paragraph {
    let (mut w, mut h) = (img.bbox.width().max(1.0), img.bbox.height().max(1.0));
    let s = (text_w / w).min(text_h / h).min(1.0);
    w *= s;
    h *= s;
    let pic = Pic::new_with_dimensions(img.png.clone(), img.px_w, img.px_h).size(emu(w), emu(h));
    let p = Paragraph::new().add_run(Run::new().add_image(pic));
    let centered = (img.bbox.cx() - content.cx()).abs() < content.width() * 0.05 && img.bbox.x0 > content.x0 + 20.0;
    if centered {
        p.align(AlignmentType::Center)
    } else {
        let indent = (img.bbox.x0 - margin_left).max(0.0).min(text_w - w);
        if indent > 1.0 {
            p.indent(Some(tw(indent)), None, None, None)
        } else {
            p
        }
    }
}
