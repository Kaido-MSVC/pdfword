//! 遞迴尋找 PDF；在背景執行緒執行，可取消，並統計無法讀取而略過的項目。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// 資料夾最大深度；超過的子資料夾計入略過
pub const MAX_DEPTH: u32 = 16;

#[derive(Debug, Default)]
pub struct ScanResult {
    /// 依路徑排序（每層資料夾內排序）
    pub found: Vec<PathBuf>,
    /// 無法讀取的資料夾或項目，以及超過深度上限的子資料夾
    pub skipped: usize,
    pub cancelled: bool,
}

pub fn is_pdf(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// 掃描 `items`（檔案或資料夾）。`progress(已找到 PDF 數, 已檢查資料夾數)` 最多每 100 ms 呼叫一次。
pub fn scan(items: Vec<PathBuf>, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize)) -> ScanResult {
    let mut st = State { res: ScanResult::default(), dirs: 0, last: Instant::now(), cancel, progress: &mut progress };
    for p in items {
        if st.stop() {
            break;
        }
        if p.is_dir() {
            st.walk(&p, 0);
        } else if is_pdf(&p) {
            st.res.found.push(p);
        }
    }
    st.res.cancelled = cancel.load(Ordering::Relaxed);
    st.res
}

struct State<'a> {
    res: ScanResult,
    dirs: usize,
    last: Instant,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(usize, usize),
}

impl State<'_> {
    fn stop(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn walk(&mut self, dir: &Path, depth: u32) {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => {
                self.res.skipped += 1;
                return;
            }
        };
        self.dirs += 1;
        let mut entries = Vec::new();
        for e in rd {
            match e {
                Ok(e) => entries.push(e.path()),
                Err(_) => self.res.skipped += 1,
            }
            if self.stop() {
                return;
            }
        }
        entries.sort();
        for p in entries {
            if self.stop() {
                return;
            }
            // is_dir 會跟隨捷徑／junction；深度上限避免迴圈無限展開
            if p.is_dir() {
                if depth < MAX_DEPTH {
                    self.walk(&p, depth + 1);
                } else {
                    self.res.skipped += 1;
                }
            } else if is_pdf(&p) {
                self.res.found.push(p);
            }
            if self.last.elapsed() >= Duration::from_millis(100) {
                self.last = Instant::now();
                (self.progress)(self.res.found.len(), self.dirs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pdfword-scan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_sorted_pdfs_and_counts_skipped() {
        let d = temp_dir("basic");
        fs::write(d.join("b.PDF"), "").unwrap();
        fs::write(d.join("a.pdf"), "").unwrap();
        fs::write(d.join("note.txt"), "").unwrap();
        // 超過深度上限的資料夾計入略過
        let mut deep = d.join("sub");
        for _ in 0..=MAX_DEPTH {
            deep = deep.join("d");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too-deep.pdf"), "").unwrap();
        fs::write(d.join("sub").join("c.pdf"), "").unwrap();

        let r = scan(vec![d.clone(), d.join("missing-folder-but-not-a-dir.txt")], &AtomicBool::new(false), |_, _| {});
        let names: Vec<_> = r.found.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.pdf", "b.PDF", "c.pdf"]);
        assert_eq!(r.skipped, 1);
        assert!(!r.cancelled);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn cancel_stops_early() {
        let d = temp_dir("cancel");
        fs::write(d.join("a.pdf"), "").unwrap();
        let r = scan(vec![d.clone()], &AtomicBool::new(true), |_, _| {});
        assert!(r.cancelled);
        assert!(r.found.is_empty());
        let _ = fs::remove_dir_all(&d);
    }
}
