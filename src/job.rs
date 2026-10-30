//! 批次轉換：決定輸出路徑、以 rayon 平行轉換、支援取消與進度回報。

use crate::convert;
use crate::extract::ExtractError;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 輸出檔已存在時的處理方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// 自動加序號「檔名 (1).docx」
    Rename,
    Overwrite,
    Skip,
}

#[derive(Debug, Clone)]
pub enum Status {
    Done { output: PathBuf, pages: usize, no_text: bool },
    Skipped { output: PathBuf },
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub enum Event {
    Started(usize),
    Progress(usize, f32),
    Finished(usize, Status),
    AllDone,
}

pub struct JobHandle {
    cancel: Arc<AtomicBool>,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// 依輸入檔與設定決定每個輸出路徑；回傳 None 表示略過（已存在且設定為略過）。
/// 同一批次內不同來源若對到同一檔名，也會自動加序號避免互相覆寫。
pub fn plan_outputs(
    inputs: &[PathBuf],
    out_dir: Option<&Path>,
    conflict: Conflict,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Result<PathBuf, PathBuf>> {
    let mut reserved: HashSet<PathBuf> = HashSet::new();
    let key = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    inputs
        .iter()
        .map(|input| {
            let dir = out_dir.map(Path::to_path_buf).unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf());
            let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
            let base = dir.join(format!("{stem}.docx"));
            let taken = |p: &Path, reserved: &HashSet<PathBuf>| reserved.contains(&key(p));
            let result = match conflict {
                Conflict::Skip if exists(&base) => Err(base),
                Conflict::Overwrite | Conflict::Skip if !taken(&base, &reserved) => Ok(base),
                _ => {
                    let mut candidate = base.clone();
                    let mut n = 1;
                    while taken(&candidate, &reserved) || (conflict == Conflict::Rename && exists(&candidate)) {
                        candidate = dir.join(format!("{stem} ({n}).docx"));
                        n += 1;
                    }
                    Ok(candidate)
                }
            };
            if let Ok(p) = &result {
                reserved.insert(key(p));
            }
            result
        })
        .collect()
}

/// 在背景執行緒開始批次轉換。`on_event` 會從工作執行緒呼叫。
pub fn start(
    inputs: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    conflict: Conflict,
    threads: usize,
    on_event: impl Fn(Event) + Send + Sync + 'static,
) -> JobHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let handle = JobHandle { cancel: cancel.clone() };
    std::thread::spawn(move || {
        let outputs = plan_outputs(&inputs, out_dir.as_deref(), conflict, |p| p.exists());
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build();
        let run = || {
            inputs.par_iter().zip(outputs).enumerate().for_each(|(i, (input, output))| {
                if cancel.load(Ordering::Relaxed) {
                    on_event(Event::Finished(i, Status::Cancelled));
                    return;
                }
                let output = match output {
                    Ok(p) => p,
                    Err(existing) => {
                        on_event(Event::Finished(i, Status::Skipped { output: existing }));
                        return;
                    }
                };
                on_event(Event::Started(i));
                let progress = |f: f32| on_event(Event::Progress(i, f));
                let status = match convert::convert_file(input, &output, &cancel, &progress) {
                    Ok(o) => Status::Done { output, pages: o.pages, no_text: o.no_text },
                    Err(ExtractError::Cancelled) => Status::Cancelled,
                    Err(e) => Status::Failed(e.to_string()),
                };
                on_event(Event::Finished(i, status));
            });
        };
        match pool {
            Ok(p) => p.install(run),
            Err(_) => run(),
        }
        on_event(Event::AllDone);
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn same_folder_by_default() {
        let out = plan_outputs(&[p("C:/a/報告.pdf")], None, Conflict::Rename, |_| false);
        assert_eq!(out[0], Ok(p("C:/a/報告.docx")));
    }

    #[test]
    fn rename_when_exists_and_within_batch() {
        // 模擬 Windows 不分大小寫的檔案系統：D:/out/x.docx 已存在
        let exists = |q: &Path| q.to_string_lossy().to_lowercase().replace('\\', "/") == "d:/out/x.docx";
        let out = plan_outputs(&[p("C:/a/x.pdf"), p("C:/b/X.pdf")], Some(Path::new("D:/out")), Conflict::Rename, exists);
        assert_eq!(out[0], Ok(p("D:/out/x (1).docx")));
        assert_eq!(out[1], Ok(p("D:/out/X (2).docx")));
    }

    #[test]
    fn overwrite_and_skip() {
        let exists = |_: &Path| true;
        let ins = [p("C:/a/x.pdf"), p("C:/b/x.pdf")];
        let o = plan_outputs(&ins, Some(Path::new("D:/o")), Conflict::Overwrite, exists);
        assert_eq!(o[0], Ok(p("D:/o/x.docx")));
        // 同批次重名仍不互相覆寫
        assert_eq!(o[1], Ok(p("D:/o/x (1).docx")));
        let s = plan_outputs(&ins[..1], Some(Path::new("D:/o")), Conflict::Skip, exists);
        assert_eq!(s[0], Err(p("D:/o/x.docx")));
    }
}
