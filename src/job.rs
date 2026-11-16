//! 批次轉換：決定輸出路徑、以 rayon 平行轉換、支援取消與進度回報。

use crate::convert;
use crate::extract::ExtractError;
use crate::output::{self, Committed, Reservations};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use crate::output::Conflict;

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
        // 同批次已分配的名稱；提交時撞名重新取序號也會登記在這裡
        let res = Reservations::default();
        let outputs = output::plan_outputs(&inputs, out_dir.as_deref(), conflict, &res, output::path_exists);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build();
        let run = || {
            inputs.par_iter().zip(outputs).enumerate().for_each(|(i, (input, output))| {
                if cancel.load(Ordering::Relaxed) {
                    on_event(Event::Finished(i, Status::Cancelled));
                    return;
                }
                let target = match output {
                    Ok(t) => t,
                    Err(existing) => {
                        on_event(Event::Finished(i, Status::Skipped { output: existing }));
                        return;
                    }
                };
                on_event(Event::Started(i));
                let progress = |f: f32| on_event(Event::Progress(i, f));
                let status = match convert::convert_file(input, &target, &res, &cancel, &progress) {
                    Ok((o, Committed::Saved(output))) => Status::Done { output, pages: o.pages, no_text: o.no_text },
                    Ok((_, Committed::Skipped(output))) => Status::Skipped { output },
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
