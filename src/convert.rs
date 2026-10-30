//! 單一檔案的完整轉換流程：擷取 → 版面分析 → DOCX。

use crate::extract::{self, ExtractError};
use crate::{layout, writer};
use rayon::prelude::*;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Outcome {
    pub pages: usize,
    /// 文件幾乎沒有文字層（可能是掃描檔）
    pub no_text: bool,
}

/// 轉換 `input` 並寫入 `output`。先寫暫存檔，完成後才改名，取消或失敗不會留下殘檔。
/// `progress(0.0..=1.0)` 回報此檔進度。
pub fn convert_file(
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<Outcome, ExtractError> {
    // 擷取佔 70%，版面分析與寫檔佔 30%
    let doc = extract::extract_document(input, cancel, |done, total| {
        progress(0.7 * done as f32 / total.max(1) as f32)
    })?;
    let body = layout::body_size(&doc.pages);
    let layouts: Vec<_> = doc.pages.par_iter().map(|p| layout::analyze(p, body)).collect();
    if cancel.load(Ordering::Relaxed) {
        return Err(ExtractError::Cancelled);
    }
    progress(0.85);

    let glyphs: usize = doc.pages.iter().map(|p| p.glyphs.len()).sum();
    let tmp = temp_path(output);
    let result = (|| {
        let f = fs::File::create(&tmp).map_err(|e| ExtractError::Other(format!("無法建立輸出檔：{e}")))?;
        writer::write_docx(&layouts, body, BufWriter::new(f)).map_err(ExtractError::Other)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(ExtractError::Cancelled);
        }
        fs::rename(&tmp, output).map_err(|e| ExtractError::Other(format!("無法寫入輸出檔：{e}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    progress(1.0);
    Ok(Outcome { pages: doc.pages.len(), no_text: glyphs < 10 * doc.pages.len().max(1) && !doc.pages.is_empty() })
}

fn temp_path(output: &Path) -> PathBuf {
    let name = output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    output.with_file_name(format!(".{name}.{}.tmp.docx", std::process::id()))
}
