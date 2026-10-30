//! 與 PDFium 無關的頁面資料模型。座標單位為 pt，原點在頁面左上角，y 向下。

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    /// 兩矩形是否相交（含 `pad` 容差）
    pub fn intersects(&self, o: &Rect, pad: f32) -> bool {
        self.x0 - pad <= o.x1 && o.x0 - pad <= self.x1 && self.y0 - pad <= o.y1 && o.y0 - pad <= self.y1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub bbox: Rect,
    pub baseline: f32,
    pub size: f32,
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
    /// PDF 中此字元前有明確的空白字元
    pub space_before: bool,
}

#[derive(Debug, Clone)]
pub struct ImageItem {
    pub bbox: Rect,
    pub png: Vec<u8>,
    pub px_w: u32,
    pub px_h: u32,
}

/// 水平或垂直的線段（表格框線候選）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ruling {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Ruling {
    pub fn horizontal(y: f32, xa: f32, xb: f32) -> Self {
        Ruling { x0: xa.min(xb), y0: y, x1: xa.max(xb), y1: y }
    }
    pub fn vertical(x: f32, ya: f32, yb: f32) -> Self {
        Ruling { x0: x, y0: ya.min(yb), x1: x, y1: ya.max(yb) }
    }
    pub fn is_horizontal(&self) -> bool {
        (self.y1 - self.y0).abs() < (self.x1 - self.x0).abs()
    }
}

/// 非框線的向量圖形（用於偵測圖表等需點陣化的區域）
#[derive(Debug, Clone, Copy)]
pub struct VectorShape {
    pub bbox: Rect,
    /// 只由水平/垂直邊組成的填色矩形（長條圖、網底等）
    pub rect_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PageData {
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<Glyph>,
    pub images: Vec<ImageItem>,
    pub rulings: Vec<Ruling>,
    /// 非白色的填色矩形（表格網底）
    pub fills: Vec<(Rect, [u8; 3])>,
}

// ---- 版面分析結果 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
    /// 此 Run 代表一個 Tab（text 為空）
    pub tab: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub align: Align,
    /// 1..=3 表示標題層級
    pub heading: Option<u8>,
    /// 左縮排（pt，相對於所在欄/儲存格左緣）
    pub indent_left: f32,
    /// 首行縮排（pt，可為負值表示凸排）
    pub indent_first: f32,
    /// 段前距（pt）
    pub space_before: f32,
    /// 主要字級（pt）
    pub size: f32,
}

impl Paragraph {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| if r.tab { "\t" } else { r.text.as_str() }).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    None,
    Restart,
    Continue,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub col_span: usize,
    pub vmerge: VMerge,
    pub shading: Option<[u8; 3]>,
    pub valign_center: bool,
    pub paragraphs: Vec<Paragraph>,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub height: f32,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub bbox: Rect,
    pub col_widths: Vec<f32>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(ImageItem),
}

#[derive(Debug, Clone)]
pub struct PageLayout {
    pub width: f32,
    pub height: f32,
    /// 內容區域（用於推算頁邊界）
    pub content: Rect,
    pub blocks: Vec<Block>,
}
