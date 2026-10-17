//! Conversión bp (puntos PDF) ↔ px del bitmap — UNA sola implementación.
//!
//! Convenio (INTERFACES §4 y §9):
//! - `PageBitmap::scale` = px por bp = `zoom × dpr` de ventana.
//! - Origen **arriba-izquierda** en ambos espacios (hayro ya aplana la
//!   rotación de página en `render_dimensions()` y en el transform inicial,
//!   así que el mapeo es rotación-agnóstico una vez renderizado).
//! - La rotación de página NO se aplica aquí: si un PDF trae `/Rotate 90`, el
//!   bitmap ya sale girado y `page_size_pt` ya devuelve ancho/alto intercambiados.

use crate::document::PageBitmap;

/// Puntos PDF por pulgada: 1 bp = 1/72".
pub const BP_PER_INCH: f32 = 72.0;

impl PageBitmap {
    /// bp del documento (origen arriba-izquierda) → px del bitmap.
    pub fn pdf_pt_to_px(&self, x_pt: f32, y_pt: f32) -> (f32, f32) {
        (x_pt * self.scale, y_pt * self.scale)
    }

    /// px del bitmap → bp del documento (origen arriba-izquierda).
    pub fn px_to_pdf_pt(&self, x: f32, y: f32) -> (f32, f32) {
        (x / self.scale, y / self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(scale: f32) -> PageBitmap {
        PageBitmap {
            width: (612.0 * scale) as u32,
            height: (792.0 * scale) as u32,
            scale,
            rgba: Vec::new(),
        }
    }

    #[test]
    fn roundtrip_es_exacto_en_varios_escalas() {
        // dpr 1.0/1.25/1.5/2.0 con zoom 1.0/1.5/2.0 + un caso fraccionario.
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0, 1.75] {
            let bmp = bitmap(scale);
            for &(x_pt, y_pt) in &[
                (0.0_f32, 0.0_f32),
                (100.0, 200.0),
                (30.5, 42.25),
                (611.5, 791.5),
            ] {
                let (x_px, y_px) = bmp.pdf_pt_to_px(x_pt, y_pt);
                let (x_back, y_back) = bmp.px_to_pdf_pt(x_px, y_px);
                assert!(
                    (x_back - x_pt).abs() <= 0.01 && (y_back - y_pt).abs() <= 0.01,
                    "escala {scale}: ({x_pt},{y_pt}) → ({x_px},{y_px}) → ({x_back},{y_back})"
                );
            }
        }
    }

    #[test]
    fn ejemplo_del_skill_a4_zoom_1_5_dpr_2() {
        // A4 595×842 bp, zoom 1.5, dpr 2.0 → scale 3.0; (100,200) bp → (300,600) px.
        let bmp = bitmap(3.0);
        assert_eq!(bmp.pdf_pt_to_px(100.0, 200.0), (300.0, 600.0));
        assert_eq!(bmp.px_to_pdf_pt(300.0, 600.0), (100.0, 200.0));
    }
}
