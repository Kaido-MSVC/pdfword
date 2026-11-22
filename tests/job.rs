//! 批次轉換整合測試（需要 vendor/pdfium/pdfium.dll，先執行 scripts/fetch-pdfium.ps1）

mod common;

use common::root;
use pdfword::extract::{self, ExtractError};
use pdfword::job::{self, Conflict, Event, Status};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

/// 初始化 PDFium 並建立暫存輸出資料夾；`test` 為測試名稱
fn setup(test: &str) -> Option<PathBuf> {
    if !common::init_pdfium(test) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!(
        "pdfword-test-{test}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir(&dir).expect("無法建立測試專用輸出資料夾");
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

/// 產生兩頁空白 PDF。第二頁可指定裁切框，以測試取頁後才發生的錯誤。
fn two_page_pdf(second_crop_box: &str) -> String {
    let second_page =
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]{second_crop_box} >>");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        &second_page,
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let id = i + 1;
        writeln!(pdf, "{id} 0 obj\n{object}\nendobj").expect("寫入 String");
    }
    let xref = pdf.len();
    writeln!(pdf, "xref\n0 5\n0000000000 65535 f ").expect("寫入 String");
    for offset in offsets {
        writeln!(pdf, "{offset:010} 00000 n ").expect("寫入 String");
    }
    writeln!(
        pdf,
        "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    )
    .expect("寫入 String");
    pdf
}

#[test]
fn page_error_and_cancellation_allow_next_conversion() {
    let Some(out) = setup("page_error_and_cancellation_allow_next_conversion") else {
        return;
    };
    let valid = out.join("兩頁.pdf");
    let invalid = out.join("第二頁無效.pdf");
    std::fs::write(&valid, two_page_pdf("")).unwrap();
    // 裁切框與媒體框不相交，第二頁的有效尺寸為零。
    std::fs::write(&invalid, two_page_pdf(" /CropBox [700 810 710 820]")).unwrap();

    let cancel = AtomicBool::new(false);
    let mut progress = Vec::new();
    let result = extract::extract_document(&invalid, &cancel, |done, total| {
        progress.push((done, total));
    });
    let Err(ExtractError::Other(message)) = result else {
        panic!("第二頁尺寸應回傳擷取錯誤");
    };
    assert!(message.starts_with("PDF 頁面尺寸無效"), "{message}");
    assert_eq!(progress, [(1, 2)]);

    progress.clear();
    let result = extract::extract_document(&valid, &cancel, |done, total| {
        progress.push((done, total));
        cancel.store(true, Ordering::Relaxed);
    });
    assert!(matches!(result, Err(ExtractError::Cancelled)));
    assert_eq!(progress, [(1, 2)]);

    let status = run(vec![valid.clone()], &out, false);
    let [
        Some(Status::Done {
            output, pages: 2, ..
        }),
    ] = status.as_slice()
    else {
        panic!("清理後應能正常轉換兩頁：{status:?}");
    };
    assert!(std::fs::metadata(output).unwrap().len() > 1000);
    assert_eq!(leftovers(&out), Vec::<String>::new());
    for path in [&valid, &invalid, output] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(out).unwrap();
}
