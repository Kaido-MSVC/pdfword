//! 批次轉換整合測試（需要 vendor/pdfium/pdfium.dll，先執行 scripts/fetch-pdfium.ps1）

mod common;

use common::root;
use pdfword::job::{self, Conflict, Event, Status};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// 初始化 PDFium 並建立暫存輸出資料夾；`test` 為測試名稱
fn setup(test: &str) -> Option<PathBuf> {
    if !common::init_pdfium(test) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("pdfword-test-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    Some(dir)
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

fn run(inputs: Vec<PathBuf>, out: &Path, cancel_now: bool) -> Vec<Option<Status>> {
    let (tx, rx) = mpsc::channel();
    let n = inputs.len();
    let h = job::start(inputs, Some(out.to_path_buf()), Conflict::Rename, 4, move |e| {
        let _ = tx.send(e);
    });
    if cancel_now {
        h.cancel();
    }
    let mut status = vec![None; n];
    for e in rx {
        match e {
            Event::Finished(i, s) => status[i] = Some(s),
            Event::AllDone => break,
            _ => {}
        }
    }
    status
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp.docx"))
        .collect()
}

#[test]
fn batch_converts_and_reports_failures() {
    let Some(out) = setup("batch_converts_and_reports_failures") else { return };
    let fx = root().join("tests/fixtures");
    let bad = out.join("損毀.pdf");
    std::fs::write(&bad, b"%PDF-1.7 not really a pdf").unwrap();
    let inputs = vec![fx.join("text.pdf"), fx.join("table.pdf"), bad, fx.join("image.pdf"), fx.join("columns.pdf")];

    let st = run(inputs, &out, false);
    for (i, name) in [(0, "text"), (1, "table"), (3, "image"), (4, "columns")] {
        match &st[i] {
            Some(Status::Done { output, pages, .. }) => {
                assert!(output.ends_with(format!("{name}.docx")));
                assert!(std::fs::metadata(output).unwrap().len() > 1000);
                assert_eq!(*pages, 1);
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    assert!(matches!(&st[2], Some(Status::Failed(_))), "{:?}", st[2]);
    assert!(leftovers(&out).is_empty());

    // 再轉一次：已存在的檔案不被覆寫，改名輸出
    let st = run(vec![fx.join("text.pdf")], &out, false);
    match &st[0] {
        Some(Status::Done { output, .. }) => assert!(output.ends_with("text (1).docx")),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn cancel_leaves_no_partial_files() {
    let Some(out) = setup("cancel_leaves_no_partial_files") else { return };
    let fx = root().join("tests/fixtures");
    let inputs: Vec<PathBuf> = (0..8).map(|_| fx.join("columns.pdf")).collect();
    let st = run(inputs, &out, true);
    assert!(st.iter().all(|s| s.is_some()));
    assert!(st.iter().any(|s| matches!(s, Some(Status::Cancelled))));
    assert!(leftovers(&out).is_empty());
    let _ = std::fs::remove_dir_all(&out);
}
