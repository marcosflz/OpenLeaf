//! Tests de la fixture multipágina M1-C1 (virtualización, caché, medición).
//!
//! `fixtures/pdf/multipage.pdf` (30 páginas, generada con
//! `fixtures/pdf/multipage-src/multipage.tex`) es inmutable: solo se lee.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lsx_pdf::{CacheKey, PdfCache, PdfRenderer, scale_bucket};

fn multipage_pdf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/multipage.pdf")
}

fn open_renderer(cache_mb: u32) -> PdfRenderer {
    let bytes = std::fs::read(multipage_pdf()).expect("fixtures/pdf/multipage.pdf debe existir");
    PdfRenderer::open(bytes, 1, (cache_mb as usize) * 1024 * 1024)
        .expect("multipage.pdf debe abrir con hayro")
}

#[test]
fn treinta_paginas_a4() {
    let renderer = open_renderer(256);
    assert_eq!(renderer.page_count(), 30);
    let (w, h) = renderer.page_size_pt(0).unwrap();
    // Observado: A4 (el class option `letterpaper` no aplicó en este corpus).
    assert!((w - 595.276).abs() < 0.5, "ancho inesperado: {w} bp");
    assert!((h - 841.89).abs() < 0.5, "alto inesperado: {h} bp");
    // Todas las páginas son uniformes (premisa de la lista virtualizada).
    for (i, (pw, ph)) in renderer.page_sizes_pt().iter().enumerate() {
        assert!(
            (pw - 595.276).abs() < 0.5 && (ph - 841.89).abs() < 0.5,
            "página {} mide {pw}×{ph} bp",
            i + 1
        );
    }
}

#[test]
fn render_en_el_medio_y_ultima_pagina() {
    let renderer = open_renderer(256);
    for page in [0_u32, 15, 29] {
        let bmp = renderer.render_page(page, 1.0).expect("render");
        assert_eq!(bmp.width, 595);
        assert_eq!(bmp.height, 841);
        assert_eq!(bmp.rgba.len(), 595 * 841 * 4);
    }
    match renderer.render_page(30, 1.0) {
        Err(lsx_pdf::PdfError::PageOutOfRange { page, count }) => {
            assert_eq!(page, 30);
            assert_eq!(count, 30);
        }
        other => panic!("se esperaba PageOutOfRange, fue {other:?}"),
    }
}

#[test]
fn la_cache_reutiliza_y_evicta_por_presupuesto() {
    let renderer = open_renderer(256);
    let first = renderer.render_page(10, 1.0).expect("render");
    let again = renderer.render_page(10, 1.0).expect("render repetido");
    assert!(Arc::ptr_eq(&first, &again), "repetir debe venir de caché");

    // Presupuesto para una sola página (~1.9 MB por página a escala 1.0).
    let small = PdfCache::new(2 * 1024 * 1024);
    let key = |page: u32| CacheKey {
        generation: 1,
        page,
        scale_milli: scale_bucket(1.0),
    };
    small.insert(key(0), first);
    let second = renderer.render_page(1, 1.0).expect("render");
    small.insert(key(1), second);
    assert!(small.contains(&key(1)));
    assert!(
        !small.contains(&key(0)),
        "la página más vieja debe caer con presupuesto de 2 MB"
    );
}

#[test]
fn medidas_render_multipage_p50_de_5_paginas() {
    // Evidencia de presupuesto §6.3 (< 120 ms release a 100 %). Debug es más
    // lento: el número se imprime para el informe, no se asevera aquí.
    let renderer = open_renderer(256);
    let mut total = 0.0_f64;
    let mut max = 0.0_f64;
    for page in [0_u32, 7, 15, 22, 29] {
        let t0 = Instant::now();
        renderer.render_page(page, 2.0).expect("render 2.0");
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        total += ms;
        max = max.max(ms);
        println!("[m1-c2] multipage página {} @2.0: {ms:.1} ms", page + 1);
    }
    println!(
        "[m1-c2] multipage media {:.1} ms · máxima {:.1} ms (perfil de la build)",
        total / 5.0,
        max
    );
}
