#![windows_subsystem = "windows"]

use pdfword::job::{self, Conflict, Event, JobHandle, Status};
use pdfword::settings::Settings;
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use pdfword::scan;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

slint::include_modules!();

const LICENSES: &str = "\
Slint 1.18.1 — Slint Royalty-free License 2.0
PDFium（chromium/8125，bblanchon/pdfium-binaries）— BSD-3-Clause / Apache-2.0；\
內含 FreeType（FTL）、libjpeg-turbo（IJG/BSD）、OpenJPEG（BSD-2）、lcms2（MIT）、zlib、libpng、\
libtiff、Abseil（Apache-2.0）、ICU（Unicode）、HarfBuzz（MIT）、Highway（Apache-2.0）、\
dragonbox（Boost／Apache-2.0）、AGG、fast_float、simdutf、LLVM libc 等
pdfium-render 0.9.5 — MIT / Apache-2.0
docx-rs 0.4.22 — MIT
rayon 1.12.0 — MIT / Apache-2.0
rfd 0.18.0 — MIT
image 0.25 — MIT / Apache-2.0
serde、serde_json — MIT / Apache-2.0

完整授權條文請見程式目錄下的 licenses 資料夾。";

struct App {
    ui: slint::Weak<AppWindow>,
    rows: Rc<VecModel<FileRow>>,
    paths: RefCell<Vec<PathBuf>>,
    outputs: RefCell<Vec<Option<PathBuf>>>,
    settings: RefCell<Settings>,
    job: RefCell<Option<JobHandle>>,
    /// 本次批次：批次索引 → 清單列索引
    batch: RefCell<Vec<usize>>,
    quit_when_done: RefCell<bool>,
    /// 進行中的資料夾掃描（取消旗標）
    scan: RefCell<Option<Arc<AtomicBool>>>,
    /// 最近一筆實際寫出的輸出檔（開啟輸出資料夾用）
    last_output: RefCell<Option<PathBuf>>,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

fn app() -> Rc<App> {
    try_app().expect("App 尚未初始化")
}

/// 背景工作回報時視窗可能已關閉
fn try_app() -> Option<Rc<App>> {
    APP.with(|a| a.borrow().clone())
}

fn pdfium_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    #[cfg(debug_assertions)]
    if !exe_dir.join("pdfium.dll").exists() {
        return Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor").join("pdfium");
    }
    exe_dir
}

impl App {
    fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("視窗已關閉")
    }

    /// 加入檔案或資料夾。含資料夾時在背景執行緒掃描，避免大型或網路資料夾卡住視窗。
    fn add_paths(&self, items: impl IntoIterator<Item = PathBuf>) {
        let items: Vec<PathBuf> = items.into_iter().collect();
        if !items.iter().any(|p| p.is_dir()) {
            let found = items.into_iter().filter(|p| scan::is_pdf(p)).collect();
            self.add_found(found, 0, false);
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.scan.borrow_mut() = Some(cancel.clone());
        let ui = self.ui();
        ui.set_scanning(true);
        ui.set_status("正在掃描資料夾…".into());
        let weak = self.ui.clone();
        std::thread::spawn(move || {
            let progress_weak = weak.clone();
            let r = scan::scan(items, &cancel, move |found, dirs| {
                let _ = progress_weak.upgrade_in_event_loop(move |ui| {
                    ui.set_status(format!("正在掃描資料夾…已檢查 {dirs} 個資料夾，找到 {found} 個 PDF").into());
                });
            });
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_scanning(false);
                if let Some(a) = try_app() {
                    *a.scan.borrow_mut() = None;
                    a.add_found(r.found, r.skipped, r.cancelled);
                }
            });
        });
    }

    fn cancel_scan(&self) {
        if let Some(c) = self.scan.borrow().as_ref() {
            c.store(true, Ordering::Relaxed);
            self.ui().set_status("正在取消掃描…".into());
        }
    }

    /// 把找到的 PDF 加入清單（略過已在清單中的），並回報加入與略過數量
    fn add_found(&self, found: Vec<PathBuf>, skipped: usize, cancelled: bool) {
        let mut added = 0;
        {
            let mut paths = self.paths.borrow_mut();
            let mut known: HashSet<String> = paths.iter().map(|q| q.to_string_lossy().to_lowercase()).collect();
            for p in found {
                if !known.insert(p.to_string_lossy().to_lowercase()) {
                    continue;
                }
                self.rows.push(FileRow {
                    name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into(),
                    folder: p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default().into(),
                    status: "等待轉換".into(),
                    progress: 0.0,
                    state: 0,
                });
                paths.push(p);
                self.outputs.borrow_mut().push(None);
                added += 1;
            }
        }
        let mut msg = if cancelled {
            format!("已取消掃描，加入 {added} 個 PDF 檔案")
        } else if added > 0 {
            format!("已加入 {added} 個 PDF 檔案")
        } else {
            "沒有找到新的 PDF 檔案".to_string()
        };
        if skipped > 0 {
            msg += &format!("（已略過 {skipped} 個無法讀取或層數過深的項目）");
        }
        self.ui().set_status(format!("{msg}。").into());
    }

    fn remove(&self, i: usize) {
        if i < self.rows.row_count() {
            self.rows.remove(i);
            self.paths.borrow_mut().remove(i);
            self.outputs.borrow_mut().remove(i);
        }
        self.ui().set_selected_index(-1);
    }

    fn clear(&self) {
        self.rows.set_vec(Vec::new());
        self.paths.borrow_mut().clear();
        self.outputs.borrow_mut().clear();
        self.last_output.replace(None);
        let ui = self.ui();
        ui.set_selected_index(-1);
        ui.set_total_progress(0.0);
        ui.set_can_open_output(false);
    }

    fn save_settings_from_ui(&self) {
        let ui = self.ui();
        let mut s = self.settings.borrow_mut();
        s.theme = ui.get_theme();
        s.conflict = ui.get_conflict_mode();
        s.threads = ui.get_threads();
        s.save();
    }

    fn set_row(&self, i: usize, f: impl FnOnce(&mut FileRow)) {
        if let Some(mut r) = self.rows.row_data(i) {
            f(&mut r);
            self.rows.set_row_data(i, r);
        }
    }

    fn update_total(&self) {
        let batch = self.batch.borrow();
        if batch.is_empty() {
            return;
        }
        let sum: f32 = batch
            .iter()
            .filter_map(|&i| self.rows.row_data(i))
            .map(|r| if r.state >= 2 { 1.0 } else { r.progress })
            .sum();
        self.ui().set_total_progress(sum / batch.len() as f32);
    }

    fn start(&self) {
        let todo: Vec<usize> = (0..self.rows.row_count())
            .filter(|&i| self.rows.row_data(i).is_some_and(|r| r.state != 2))
            .collect();
        let ui = self.ui();
        if todo.is_empty() {
            ui.set_status("清單中的檔案都已轉換完成。".into());
            return;
        }
        for &i in &todo {
            self.set_row(i, |r| {
                r.state = 0;
                r.progress = 0.0;
                r.status = "等待轉換".into();
            });
        }
        let inputs: Vec<PathBuf> = todo.iter().map(|&i| self.paths.borrow()[i].clone()).collect();
        *self.batch.borrow_mut() = todo;
        let s = self.settings.borrow().clone();
        let conflict = match ui.get_conflict_mode() {
            1 => Conflict::Overwrite,
            2 => Conflict::Skip,
            _ => Conflict::Rename,
        };
        ui.set_running(true);
        ui.set_total_progress(0.0);
        ui.set_status(format!("正在轉換 {} 個檔案…", inputs.len()).into());

        let weak = self.ui.clone();
        let handle = job::start(inputs, s.output_dir.clone(), conflict, ui.get_threads().max(1) as usize, move |e| {
            let _ = weak.upgrade_in_event_loop(move |_| app().on_event(e));
        });
        *self.job.borrow_mut() = Some(handle);
    }

    fn on_event(&self, e: Event) {
        let row = |k: usize| self.batch.borrow().get(k).copied();
        let all_done = matches!(e, Event::AllDone);
        match e {
            Event::Started(k) => {
                if let Some(i) = row(k) {
                    self.set_row(i, |r| {
                        r.state = 1;
                        r.status = "轉換中…".into();
                    });
                }
            }
            Event::Progress(k, f) => {
                if let Some(i) = row(k) {
                    self.set_row(i, |r| {
                        r.progress = f;
                        r.status = format!("轉換中 {:.0}%", f * 100.0).into();
                    });
                }
            }
            Event::Finished(k, st) => {
                let Some(i) = row(k) else { return };
                let (state, text, output) = match st {
                    Status::Done { output, pages, no_text } => {
                        let name = output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let text = if no_text {
                            format!("完成（無文字層，未做 OCR）→ {name}")
                        } else {
                            format!("完成 {pages} 頁 → {name}")
                        };
                        (2, text, Some(output))
                    }
                    Status::Skipped { output } => (4, "略過（輸出檔已存在）".to_string(), Some(output)),
                    Status::Failed(msg) => (3, format!("失敗：{msg}"), None),
                    Status::Cancelled => (5, "已取消".to_string(), None),
                };
                if state == 2 {
                    self.last_output.replace(output.clone());
                }
                self.outputs.borrow_mut()[i] = output;
                self.set_row(i, |r| {
                    r.state = state;
                    r.progress = if state == 2 { 1.0 } else { r.progress };
                    r.status = SharedString::from(text);
                });
            }
            Event::AllDone => {
                *self.job.borrow_mut() = None;
                let ui = self.ui();
                ui.set_running(false);
                let states: Vec<i32> = self.batch.borrow().iter().filter_map(|&i| self.rows.row_data(i)).map(|r| r.state).collect();
                let count = |s: i32| states.iter().filter(|&&x| x == s).count();
                let mut msg = format!("完成 {} 個", count(2));
                if count(3) > 0 {
                    msg += &format!("，失敗 {} 個", count(3));
                }
                if count(4) > 0 {
                    msg += &format!("，略過 {} 個", count(4));
                }
                if count(5) > 0 {
                    msg += &format!("，取消 {} 個", count(5));
                }
                ui.set_status(format!("{msg}。").into());
                ui.set_can_open_output(self.outputs.borrow().iter().any(Option::is_some));
                if *self.quit_when_done.borrow() {
                    let _ = slint::quit_event_loop();
                }
            }
        }
        if !all_done {
            self.update_total();
        }
    }

    /// 開啟實際輸出檔所在的資料夾（不是目前的設定）：優先選取的列，其次最近一筆完成的輸出
    fn open_output_folder(&self) {
        let outputs = self.outputs.borrow();
        let selected = usize::try_from(self.ui().get_selected_index()).ok().and_then(|i| outputs.get(i).cloned().flatten());
        let target = selected.or_else(|| self.last_output.borrow().clone()).or_else(|| outputs.iter().rev().flatten().next().cloned());
        let Some(file) = target else { return };
        if file.exists() {
            // 開啟資料夾並選取該檔；Windows 路徑不會含引號，可直接包起來
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", file.display())).spawn();
        } else if let Some(dir) = file.parent().filter(|d| d.is_dir()) {
            let _ = std::process::Command::new("explorer").arg(dir).spawn();
        } else {
            self.ui().set_status("找不到輸出檔所在的資料夾，可能已被移動或刪除。".into());
        }
    }
}

/// 讓 Windows 標題列跟著主題設定（0 跟隨系統）
fn apply_titlebar_theme(ui: &AppWindow) {
    let theme = match ui.get_theme() {
        1 => Some(winit::window::Theme::Light),
        2 => Some(winit::window::Theme::Dark),
        _ => None,
    };
    ui.window().with_winit_window(|w| w.set_theme(theme));
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let settings = Settings::load();
    let rows = Rc::new(VecModel::<FileRow>::default());
    ui.set_files(ModelRc::from(rows.clone()));
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_licenses(LICENSES.into());
    ui.set_theme(settings.theme);
    ui.invoke_apply_theme();
    ui.set_conflict_mode(settings.conflict.clamp(0, 2));
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16) as i32;
    ui.set_max_threads(cpus);
    ui.set_threads(settings.threads.clamp(1, cpus));
    ui.set_output_dir(settings.output_dir.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default().into());

    if let Err(e) = pdfword::extract::init(&pdfium_dir()) {
        ui.set_ready(false);
        ui.set_status(format!("{e}。請確認 pdfium.dll 與程式放在同一個資料夾。").into());
    }

    let state = Rc::new(App {
        ui: ui.as_weak(),
        rows,
        paths: RefCell::default(),
        outputs: RefCell::default(),
        settings: RefCell::new(settings),
        job: RefCell::default(),
        batch: RefCell::default(),
        quit_when_done: RefCell::new(false),
        scan: RefCell::default(),
        last_output: RefCell::default(),
    });
    APP.with(|a| *a.borrow_mut() = Some(state));

    // 命令列參數（拖放到 exe 圖示、「開啟檔案」）在事件迴圈開始後加入清單
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if !args.is_empty() {
        slint::Timer::single_shot(std::time::Duration::ZERO, move || app().add_paths(args));
    }

    ui.on_add_files(|| {
        let a = app();
        let mut dlg = rfd::FileDialog::new().set_title("選擇 PDF 檔案").add_filter("PDF 檔案", &["pdf", "PDF"]);
        if let Some(d) = &a.settings.borrow().last_input_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(files) = dlg.pick_files() {
            if let Some(dir) = files.first().and_then(|f| f.parent()) {
                let mut s = a.settings.borrow_mut();
                s.last_input_dir = Some(dir.to_path_buf());
                s.save();
            }
            a.add_paths(files);
        }
    });
    ui.on_add_folder(|| {
        let a = app();
        let mut dlg = rfd::FileDialog::new().set_title("選擇包含 PDF 的資料夾");
        if let Some(d) = &a.settings.borrow().last_input_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(dir) = dlg.pick_folder() {
            {
                let mut s = a.settings.borrow_mut();
                s.last_input_dir = Some(dir.clone());
                s.save();
            }
            a.add_paths([dir]);
        }
    });
    ui.on_remove_selected(|| {
        let a = app();
        let i = a.ui().get_selected_index();
        if i >= 0 {
            a.remove(i as usize);
        }
    });
    ui.on_clear_list(|| app().clear());
    ui.on_choose_output(|| {
        let a = app();
        let mut dlg = rfd::FileDialog::new().set_title("選擇輸出資料夾");
        let current = a.settings.borrow().output_dir.clone().or_else(|| a.settings.borrow().last_input_dir.clone());
        if let Some(d) = current {
            dlg = dlg.set_directory(d);
        }
        if let Some(dir) = dlg.pick_folder() {
            a.ui().set_output_dir(dir.to_string_lossy().into_owned().into());
            let mut s = a.settings.borrow_mut();
            s.output_dir = Some(dir);
            s.save();
        }
    });
    ui.on_reset_output(|| {
        let a = app();
        a.ui().set_output_dir(SharedString::new());
        let mut s = a.settings.borrow_mut();
        s.output_dir = None;
        s.save();
    });
    ui.on_start(|| app().start());
    ui.on_cancel(|| {
        let a = app();
        if let Some(j) = a.job.borrow().as_ref() {
            j.cancel();
            a.ui().set_status("正在取消…".into());
        }
    });
    ui.on_cancel_scan(|| app().cancel_scan());
    ui.on_open_output(|| app().open_output_folder());
    ui.on_open_file(|i| {
        let a = app();
        let out = a.outputs.borrow().get(i as usize).cloned().flatten();
        let target = out.filter(|p| p.exists()).or_else(|| a.paths.borrow().get(i as usize).cloned());
        if let Some(p) = target {
            let _ = std::process::Command::new("explorer").arg(p).spawn();
        }
    });
    ui.on_settings_changed(|| {
        let a = app();
        a.save_settings_from_ui();
        apply_titlebar_theme(&a.ui());
    });
    ui.on_quit(|| {
        let a = app();
        if a.job.borrow().is_some() {
            a.ui().window().dispatch_event(slint::platform::WindowEvent::CloseRequested);
        } else {
            let _ = slint::quit_event_loop();
        }
    });

    // 轉換中關閉視窗：先取消，等工作結束再離開，避免留下暫存檔
    ui.window().on_close_requested(|| {
        let a = app();
        if let Some(c) = a.scan.borrow().as_ref() {
            c.store(true, Ordering::Relaxed);
        }
        if let Some(j) = a.job.borrow().as_ref() {
            j.cancel();
            *a.quit_when_done.borrow_mut() = true;
            a.ui().set_status("正在取消，完成後會自動關閉…".into());
            return CloseRequestResponse::KeepWindowShown;
        }
        CloseRequestResponse::HideWindow
    });

    ui.show()?;
    apply_titlebar_theme(&ui);

    // 從檔案總管拖放（Slint 1.18 的 DropArea 不接受外部檔案，改用 winit 事件）
    ui.window().on_winit_window_event(|_, ev| {
        match ev {
            winit::event::WindowEvent::HoveredFile(_) => {
                let a = app();
                if !a.ui().get_running() && !a.ui().get_scanning() {
                    a.ui().set_drag_hover(true);
                }
            }
            winit::event::WindowEvent::HoveredFileCancelled => app().ui().set_drag_hover(false),
            winit::event::WindowEvent::DroppedFile(path) => {
                let a = app();
                let ui = a.ui();
                ui.set_drag_hover(false);
                if ui.get_running() {
                    ui.set_status("轉換進行中，請完成後再加入檔案。".into());
                } else if ui.get_scanning() {
                    ui.set_status("正在掃描資料夾，請完成或取消後再加入檔案。".into());
                } else {
                    a.add_paths([path.clone()]);
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });

    slint::run_event_loop()?;
    APP.with(|a| *a.borrow_mut() = None);
    Ok(())
}
