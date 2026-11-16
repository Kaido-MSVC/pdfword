//! 單一檔案的完整轉換流程：擷取 → 版面分析 → DOCX。

use crate::extract::{self, ExtractError};
use crate::output::{self, Committed, Reservations, Target, TempFile};
use crate::{layout, writer};
use rayon::prelude::*;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Outcome {
    pub pages: usize,
    /// 文件幾乎沒有文字層（可能是掃描檔）
    pub no_text: bool,
}

/// 轉換 `input` 並依 `target` 的撞名規則提交。先寫同資料夾的暫存檔，
/// 完成後以不覆寫的方式落檔（覆寫模式除外）；取消、失敗或略過都不會留下暫存檔。
/// `progress(0.0..=1.0)` 回報此檔進度。
pub fn convert_file(
    input: &Path,
    target: &Target,
    res: &Reservations,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<(Outcome, Committed), ExtractError> {
    // 擷取佔 70%，版面分析與寫檔佔 30%
    let doc = extract::extract_document(input, cancel, |done, total| {
        progress(0.7 * done as f32 / total.max(1) as f32)
    })?;
    let body = layout::body_size(&doc.pages);
    let running = layout::running_lines(&doc.pages);
    let mut layouts: Vec<_> = doc.pages.par_iter().map(|p| layout::analyze_with(p, body, &running)).collect();
    layout::merge_across_pages(&mut layouts);
    if cancel.load(Ordering::Relaxed) {
        return Err(ExtractError::Cancelled);
    }
    progress(0.85);

    let glyphs: usize = doc.pages.iter().map(|p| p.glyphs.len()).sum();
    // TempFile 在提交前被丟棄（任何 ? 提早返回）時會自動刪除
    let (tmp, f) = TempFile::create_near(&target.path).map_err(|e| ExtractError::Other(format!("無法建立輸出檔：{e}")))?;
    writer::write_docx(&layouts, body, BufWriter::new(f)).map_err(ExtractError::Other)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(ExtractError::Cancelled);
    }
    let committed = output::commit(tmp, target, res, output::path_exists)
        .map_err(|e| ExtractError::Other(format!("無法寫入輸出檔：{e}")))?;
    progress(1.0);
    let outcome = Outcome { pages: doc.pages.len(), no_text: glyphs < 10 * doc.pages.len().max(1) && !doc.pages.is_empty() };
    Ok((outcome, committed))
}
