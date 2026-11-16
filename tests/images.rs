//! 圖片旋轉、鏡射與遮罩（需要 vendor/pdfium/pdfium.dll）
//!
//! 測試 PDF 在執行時手寫產生：同一張 4×2 圖（左半紅、右半藍）以四種方式繪製。

mod common;

use std::sync::atomic::AtomicBool;

/// 產生含 4 個圖片物件的單頁 PDF（612×792）
fn build_pdf() -> Vec<u8> {
    let row: Vec<u8> = [[255u8, 0, 0], [255, 0, 0], [0, 0, 255], [0, 0, 255]].concat();
    let rgb = [row.clone(), row].concat();
    let mask = vec![255u8, 255, 0, 0, 255, 255, 0, 0];
    let content = b"q 40 0 0 20 100 700 cm /Im1 Do Q\n\
q 0 -40 20 0 100 600 cm /Im1 Do Q\n\
q -40 0 0 20 140 500 cm /Im1 Do Q\n\
q 40 0 0 20 100 400 cm /Im2 Do Q\n"
        .to_vec();
    let img = |extra: &str, data: &[u8]| {
        let mut v = format!(
            "<< /Type /XObject /Subtype /Image /Width 4 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8{extra} /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        v.extend_from_slice(data);
        v.extend_from_slice(b"\nendstream");
        v
    };
    let mut smask = format!(
        "<< /Type /XObject /Subtype /Image /Width 4 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
        mask.len()
    )
    .into_bytes();
    smask.extend_from_slice(&mask);
    smask.extend_from_slice(b"\nendstream");
    let mut contents = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    contents.extend_from_slice(&content);
    contents.extend_from_slice(b"\nendstream");

    let objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Im1 4 0 R /Im2 5 0 R >> >> /Contents 7 0 R >>".to_vec(),
        img("", &rgb),
        img(" /SMask 6 0 R", &rgb),
        smask,
        contents,
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn is_red(p: &image::Rgba<u8>) -> bool {
    p[0] > 200 && p[2] < 60 && p[3] > 200
}

fn is_blue(p: &image::Rgba<u8>) -> bool {
    p[2] > 200 && p[0] < 60 && p[3] > 200
}

#[test]
fn rotated_mirrored_and_masked_images() {
    if !common::init_pdfium("rotated_mirrored_and_masked_images") {
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdfword-images-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("xform.pdf");
    std::fs::write(&pdf, build_pdf()).unwrap();
    let doc = pdfword::extract::extract_document(&pdf, &AtomicBool::new(false), |_, _| {}).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let mut imgs = doc.pages[0].images.clone();
    imgs.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
    assert_eq!(imgs.len(), 4, "{:?}", imgs.iter().map(|i| i.bbox).collect::<Vec<_>>());
    let decode = |k: usize| image::load_from_memory(&imgs[k].png).unwrap().to_rgba8();

    // 直立：左紅右藍
    let a = decode(0);
    assert_eq!(a.dimensions(), (4, 2));
    assert!(is_red(a.get_pixel(0, 0)) && is_blue(a.get_pixel(3, 1)));

    // 順時針 90°：外框 20×40，上紅下藍
    let b = decode(1);
    assert!((imgs[1].bbox.width() - 20.0).abs() < 0.5 && (imgs[1].bbox.height() - 40.0).abs() < 0.5);
    assert_eq!(b.dimensions(), (2, 4));
    assert!(is_red(b.get_pixel(0, 0)) && is_blue(b.get_pixel(1, 3)), "{:?} {:?}", b.get_pixel(0, 0), b.get_pixel(1, 3));

    // 水平鏡射：左藍右紅
    let c = decode(2);
    assert_eq!(c.dimensions(), (4, 2));
    assert!(is_blue(c.get_pixel(0, 0)) && is_red(c.get_pixel(3, 1)), "{:?} {:?}", c.get_pixel(0, 0), c.get_pixel(3, 1));

    // SMask：左半不透明紅、右半透明
    let d = decode(3);
    assert!(is_red(d.get_pixel(0, 0)), "{:?}", d.get_pixel(0, 0));
    assert!(d.get_pixel(3, 1)[3] < 30, "右半應透明：{:?}", d.get_pixel(3, 1));
}
