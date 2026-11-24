//! Peças de geometria para uma futura integração móvel do launcher original.
//!
//! As coordenadas são pixels lógicos GPUI, não pixels físicos. Este módulo ainda
//! não está ligado ao renderizador nem a eventos; não modifica o desktop.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }

    fn intersection(self, other: Self) -> Option<Self> {
        if !self.valid() || !other.valid() {
            return None;
        }
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let result = Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };
        result.valid().then_some(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PopupLayout {
    pub bounds: Rect,
    /// O conteúdo precisa de uma área de rolagem vertical para não ser cortado.
    pub scroll_y: bool,
    /// O chamador deve ajustar/rolar horizontalmente se o conteúdo não puder quebrar.
    pub clipped_x: bool,
}

/// Limita o popover existente à área útil e informa quando precisa de rolagem.
/// `preferred` contém a posição e o tamanho normalmente escolhidos pelo launcher;
/// `viewport` já deve excluir recortes, barras do sistema e teclado Android.
/// Retorna None para medidas inválidas ou se a margem deixa área zero.
pub(crate) fn fit_popup(viewport: Rect, preferred: Rect, margin: f32) -> Option<PopupLayout> {
    if !viewport.valid() || !preferred.valid() || !margin.is_finite() || margin < 0.0 {
        return None;
    }
    let inner = Rect {
        x: viewport.x + margin,
        y: viewport.y + margin,
        width: viewport.width - 2.0 * margin,
        height: viewport.height - 2.0 * margin,
    };
    if !inner.valid() {
        return None;
    }
    let width = preferred.width.min(inner.width);
    let height = preferred.height.min(inner.height);
    let bounds = Rect {
        x: preferred.x.clamp(inner.x, inner.x + (inner.width - width)),
        y: preferred
            .y
            .clamp(inner.y, inner.y + (inner.height - height)),
        width,
        height,
    };
    bounds.valid().then_some(PopupLayout {
        bounds,
        scroll_y: preferred.height > height,
        clipped_x: preferred.width > width,
    })
}

/// Amplia o alvo de um controle sem alterar o desenho visual.
/// `lane` é a região exclusiva alocada ao controle, não sobreposta a outros;
/// `viewport` já exclui insets e teclado. O resultado é recortado aos dois.
/// O alvo mínimo é apenas uma meta: numa faixa menor, o resultado será menor.
/// Não faz despacho de eventos, captura de gesto ou prevenção de scroll.
pub(crate) fn touch_bounds(visual: Rect, lane: Rect, viewport: Rect, minimum: f32) -> Option<Rect> {
    if !visual.valid() || !minimum.is_finite() || minimum <= 0.0 {
        return None;
    }
    let allowed = lane.intersection(viewport)?;
    // Se o controle não intersecta a faixa, não criar uma região fantasma.
    visual.intersection(allowed)?;
    let width = visual.width.max(minimum);
    let height = visual.height.max(minimum);
    let expanded = Rect {
        x: visual.x - (width - visual.width) / 2.0,
        y: visual.y - (height - visual.height) / 2.0,
        width,
        height,
    };
    expanded.intersection(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    fn inside(a: Rect, b: Rect) -> bool {
        a.x >= b.x
            && a.y >= b.y
            && a.x + a.width <= b.x + b.width
            && a.y + a.height <= b.y + b.height
    }

    #[test]
    fn popup_preserves_desktop_sized_position_when_it_fits() {
        let preferred = r(100., 80., 260., 180.);
        assert_eq!(
            fit_popup(r(0., 0., 1280., 720.), preferred, 8.),
            Some(PopupLayout {
                bounds: preferred,
                scroll_y: false,
                clipped_x: false
            })
        );
    }
    #[test]
    fn popup_moves_back_from_bottom_and_right_edges() {
        let got = fit_popup(r(0., 0., 360., 640.), r(300., 620., 200., 250.), 8.).unwrap();
        assert_eq!(got.bounds, r(152., 382., 200., 250.));
        assert!(!got.scroll_y && !got.clipped_x);
    }
    #[test]
    fn popup_fits_short_and_narrow_safe_area_and_reports_overflow() {
        let got = fit_popup(r(10., 30., 200., 140.), r(5., 20., 240., 300.), 8.).unwrap();
        assert_eq!(got.bounds, r(18., 38., 184., 124.));
        assert!(got.scroll_y && got.clipped_x);
    }
    #[test]
    fn popup_flags_overflow_axes_independently() {
        let v = r(0., 0., 200., 100.);
        let a = fit_popup(v, r(0., 0., 50., 120.), 0.).unwrap();
        assert!(a.scroll_y && !a.clipped_x);
        let b = fit_popup(v, r(0., 0., 250., 20.), 0.).unwrap();
        assert!(!b.scroll_y && b.clipped_x);
    }
    #[test]
    fn popup_rejects_invalid_dimensions_margins_and_nonfinite_values() {
        let v = r(0., 0., 200., 100.);
        let p = r(0., 0., 40., 30.);
        for margin in [-1., 50., 101., f32::NAN, f32::INFINITY] {
            assert!(fit_popup(v, p, margin).is_none());
        }
        for bad in [
            r(0., 0., 0., 1.),
            r(0., 0., -1., 1.),
            r(f32::NAN, 0., 1., 1.),
            r(0., 0., f32::INFINITY, 1.),
            r(f32::MAX, 0., f32::MAX, 1.),
        ] {
            assert!(fit_popup(bad, p, 0.).is_none());
            assert!(fit_popup(v, bad, 0.).is_none());
        }
    }
    #[test]
    fn touch_target_grows_32_to_48_without_changing_visual() {
        assert_eq!(
            touch_bounds(
                r(20., 20., 32., 32.),
                r(0., 0., 100., 100.),
                r(0., 0., 100., 100.),
                48.
            ),
            Some(r(12., 12., 48., 48.))
        );
    }
    #[test]
    fn touch_target_clips_to_exclusive_lane_and_safe_viewport() {
        assert_eq!(
            touch_bounds(
                r(4., 10., 32., 32.),
                r(0., 0., 36., 80.),
                r(2., 4., 90., 90.),
                48.
            ),
            Some(r(2., 4., 34., 46.))
        );
    }
    #[test]
    fn touch_does_not_shrink_large_control() {
        let visual = r(10., 10., 70., 60.);
        assert_eq!(
            touch_bounds(visual, r(0., 0., 100., 100.), r(0., 0., 100., 100.), 48.),
            Some(visual)
        );
    }
    #[test]
    fn touch_rejects_ghost_invalid_and_empty_intersections() {
        let v = r(0., 0., 100., 100.);
        let visual = r(10., 10., 20., 20.);
        for min in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(touch_bounds(visual, v, v, min).is_none());
        }
        assert!(touch_bounds(visual, r(50., 50., 10., 10.), v, 48.).is_none());
        assert!(touch_bounds(visual, v, r(100., 100., 10., 10.), 48.).is_none());
        assert!(touch_bounds(r(f32::NAN, 0., 1., 1.), v, v, 48.).is_none());
    }
    #[test]
    fn popup_grid_stays_inside_safe_area_including_tiny_sizes() {
        for w in [1., 40., 180., 360., 1000.] {
            for h in [1., 24., 120., 640.] {
                let viewport = r(7., 13., w, h);
                for x in [-500., 0., 7., 2000.] {
                    for y in [-1000., 0., 13., 2000.] {
                        let got = fit_popup(viewport, r(x, y, 500., 700.), 0.).unwrap();
                        assert!(inside(got.bounds, viewport));
                        assert_eq!(got.clipped_x, 500. > w);
                        assert_eq!(got.scroll_y, 700. > h);
                    }
                }
            }
        }
    }
    #[test]
    fn touch_grid_never_escapes_exclusive_lane_or_viewport() {
        let viewport = r(3., 7., 300., 400.);
        for x in [0., 3., 50., 150., 290.] {
            for y in [0., 7., 80., 390.] {
                let lane = r(x, y, 30., 30.);
                let visual = r(x + 5., y + 5., 20., 20.);
                if let Some(got) = touch_bounds(visual, lane, viewport, 48.) {
                    assert!(inside(got, lane) && inside(got, viewport));
                }
            }
        }
    }
    #[test]
    fn adjacent_exclusive_lanes_never_overlap_touch_bounds() {
        let viewport = r(0., 0., 100., 100.);
        let left = touch_bounds(r(10., 10., 20., 20.), r(0., 0., 50., 50.), viewport, 48.).unwrap();
        let right =
            touch_bounds(r(60., 10., 20., 20.), r(50., 0., 50., 50.), viewport, 48.).unwrap();
        assert!(left.x + left.width <= right.x);
    }
}
