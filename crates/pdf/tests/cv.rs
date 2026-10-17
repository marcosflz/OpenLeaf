//! Tests de integración del spike M0-S3 contra `fixtures/pdf/cv.pdf`.
//!
//! La fixture es inmutable: estos tests solo la leen.

use std::path::PathBuf;
use std::time::Instant;

use lsx_pdf::{BP_PER_INCH, PdfDocument, PdfError};

fn cv_pdf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/cv.pdf")
}

fn open_cv() -> PdfDocument {
    let bytes = std::fs::read(cv_pdf()).expect("fixtures/pdf/cv.pdf debe existir");
    PdfDocument::open(bytes).expect("cv.pdf debe abrir con hayro")
}

#[test]
fn bp_per_inch_es_72() {
    assert_eq!(BP_PER_INCH, 72.0);
}

#[test]
fn cv_se_abre_y_page_size_es_letter() {
    let doc = open_cv();
    let count = doc.page_count();
    assert!(count >= 1, "cv.pdf no tiene páginas");
    let (w, h) = doc.page_size_pt(0).expect("página 0 debe existir");
    println!("[spike-s3] cv.pdf: {count} página(s); página 1 = {w}×{h} bp");
    // Letter observado (ver informe del spike; tolerancia por si se recompila).
    assert!((w - 612.0).abs() < 1.0, "ancho inesperado: {w} bp");
    assert!((h - 792.0).abs() < 1.0, "alto inesperado: {h} bp");
}

#[test]
fn roundtrip_bp_px_en_pagina_real() {
    let doc = open_cv();
    let (w_pt, h_pt) = doc.page_size_pt(0).unwrap();

    for scale in [1.0_f32, 1.5, 2.0] {
        let bmp = doc.render(0, scale).expect("render de cv.pdf");

        // hayro dimensiona con floor(ancho_bp × scale) sobre u16.
        assert_eq!(bmp.width, (w_pt * scale) as u32, "ancho @ {scale}");
        assert_eq!(bmp.height, (h_pt * scale) as u32, "alto @ {scale}");
        assert_eq!(
            bmp.rgba.len(),
            (bmp.width * bmp.height * 4) as usize,
            "bytes RGBA @ {scale}"
        );

        for &(x_pt, y_pt) in &[
            (0.0_f32, 0.0_f32),
            (100.0, 200.0),
            (30.5, 42.25),
            (w_pt - 0.5, h_pt - 0.5),
        ] {
            let (x_px, y_px) = bmp.pdf_pt_to_px(x_pt, y_pt);
            let (x_back, y_back) = bmp.px_to_pdf_pt(x_px, y_px);
            assert!(
                (x_back - x_pt).abs() <= 0.01 && (y_back - y_pt).abs() <= 0.01,
                "scale {scale}: ({x_pt},{y_pt}) → ({x_px},{y_px}) → ({x_back},{y_back})"
            );
        }
    }
}

#[test]
fn medidas_render_cv_pagina_1_media_de_5() {
    let doc = open_cv();
    for scale in [1.0_f32, 1.5, 2.0] {
        let mut times = Vec::with_capacity(5);
        for _ in 0..5 {
            let t0 = Instant::now();
            doc.render(0, scale).expect("render de cv.pdf");
            times.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "[spike-s3] render(page=1, scale={scale:.1}): media {mean:.1} ms · runs {times:?} ms"
        );
    }
}

#[test]
fn medidas_render_cache_reutilizado() {
    let doc = open_cv();
    let cache = hayro::RenderCache::new();
    for scale in [1.0_f32, 1.5, 2.0] {
        let mut times = Vec::with_capacity(5);
        for _ in 0..5 {
            let t0 = Instant::now();
            doc.render_with_cache(0, scale, &cache)
                .expect("render de cv.pdf");
            times.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "[spike-s3] render_with_cache(page=1, scale={scale:.1}): media {mean:.1} ms · runs {times:?} ms"
        );
    }
}

#[test]
fn errores_tipados() {
    let doc = open_cv();

    match doc.page_size_pt(999) {
        Err(PdfError::PageOutOfRange { page, count }) => {
            assert_eq!(page, 999);
            assert!(count >= 1);
        }
        other => panic!("se esperaba PageOutOfRange, fue {other:?}"),
    }

    match doc.render(0, 0.0) {
        Err(PdfError::InvalidScale { scale }) => assert_eq!(scale, 0.0),
        other => panic!("se esperaba InvalidScale, fue {other:?}"),
    }

    if !matches!(PdfDocument::open(Vec::new()), Err(PdfError::Open(_))) {
        panic!("se esperaba Open(_) con bytes vacíos");
    }
}
