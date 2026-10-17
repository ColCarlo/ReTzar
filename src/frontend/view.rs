//! Screen geometry. Everything is measured in physical pixels, so original
//! pixels stay square at any Windows DPI setting.
use bevy::prelude::*;
use retzar::menu::{Menu, Settings};

/// The classic interface and menus were drawn for 800×600.
pub const DESIGN: Vec2 = Vec2::new(800., 600.);

/// Window size in physical pixels and the logical-to-physical factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub physical: Vec2,
    pub dpi: f32,
}
impl Surface {
    pub fn of(w: &Window) -> Self {
        Self {
            physical: Vec2::new(w.physical_width() as f32, w.physical_height() as f32),
            dpi: w.scale_factor(),
        }
    }
    fn fit(self) -> f32 {
        (self.physical.x / DESIGN.x).min(self.physical.y / DESIGN.y)
    }
}

/// How one interface pixel maps to the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiLayout {
    /// Physical pixels per interface pixel.
    pub factor: f32,
    /// Interface size in its own pixels (at least 800×600 when it fits).
    pub size: Vec2,
    /// Physical position of the interface's top-left corner.
    pub origin: Vec2,
    /// Whole-number factor: sample the art without filtering.
    pub crisp: bool,
}
impl UiLayout {
    /// Interface pixel of a logical cursor position.
    pub fn point(&self, surface: Surface, cursor: Vec2) -> Vec2 {
        (cursor * surface.dpi - self.origin) / self.factor
    }
    /// Orthographic scale of a camera that shows interface pixels at `factor`.
    pub fn camera_scale(&self, surface: Surface) -> f32 {
        surface.dpi / self.factor
    }
    /// World position (interface camera centred on the origin) of the centre
    /// of a sprite whose top-left corner sits at interface pixel `at`.
    pub fn place(&self, surface: Surface, at: Vec2, size: Vec2) -> Vec2 {
        let top_left = (self.origin + at * self.factor - surface.physical / 2.) / self.factor;
        Vec2::new(top_left.x + size.x / 2., -(top_left.y + size.y / 2.))
    }
}

fn whole(fit: f32) -> Option<f32> {
    (fit >= 1.).then(|| fit.floor())
}

/// The mission interface covers the whole window. In crisp mode it uses the
/// largest whole factor that still leaves an 800×600 design area.
pub fn hud_layout(surface: Surface, settings: &Settings) -> UiLayout {
    let fit = surface.fit();
    let (factor, crisp) = match whole(fit).filter(|_| settings.crisp()) {
        Some(f) => (f, true),
        None => (fit.max(0.1), false),
    };
    UiLayout {
        factor,
        size: (surface.physical / factor).ceil(),
        origin: Vec2::ZERO,
        crisp,
    }
}

/// The 800×600 menu canvas, centred on whole physical pixels.
pub fn menu_layout(surface: Surface, settings: &Settings) -> UiLayout {
    let fit = surface.fit();
    let (factor, crisp) = match whole(fit).filter(|_| settings.crisp()) {
        Some(f) => (f, true),
        None => (fit.max(0.1), false),
    };
    UiLayout {
        factor,
        size: DESIGN,
        origin: ((surface.physical - DESIGN * factor) / 2.).round(),
        crisp,
    }
}

/// The menu canvas. A dialog opened over the running mission (pause,
/// save, load) shares the mission interface's scale so that the two line up;
/// one the player dragged takes the canvas, clear around it, along
/// (`Menu::canvas_shift`).
pub fn canvas_layout(surface: Surface, menu: &Menu) -> UiLayout {
    let mut layout = canvas_base(surface, menu);
    let (dx, dy) = menu.canvas_shift();
    layout.origin += (Vec2::new(dx as f32, dy as f32) * layout.factor).round();
    layout
}
/// The menu canvas where it opens.
pub fn canvas_base(surface: Surface, menu: &Menu) -> UiLayout {
    if menu.over_map() {
        let hud = hud_layout(surface, &menu.settings);
        UiLayout {
            factor: hud.factor,
            size: DESIGN,
            origin: ((surface.physical - DESIGN * hud.factor) / 2.).round(),
            crisp: hud.crisp,
        }
    } else {
        menu_layout(surface, menu.current_settings())
    }
}

/// Original framing: map pixels as large as interface pixels, as in the
/// original game where map and panels shared one resolution. Always a whole
/// number of physical pixels, so every map pixel keeps the same size.
pub fn default_zoom(hud: &UiLayout) -> f32 {
    hud.factor.round().max(1.)
}

/// The original had no zoom; this version has three levels chosen by the
/// player, as multiples of the original framing `f` (map pixels as large as
/// interface pixels): 0.5, 0.75 and 1 × `f`, i.e. 1×, 1.5× and 2× on a
/// 1440p screen where `f` is 2.
pub fn zoom_levels(hud: &UiLayout) -> [f32; 3] {
    let f = default_zoom(hud);
    [0.5 * f, 0.75 * f, f]
}

/// Opening and Home zoom: the original framing, a whole number of physical
/// pixels per map pixel.
pub fn opening_zoom(hud: &UiLayout) -> f32 {
    default_zoom(hud)
}

/// The steps of «Zoom fluido», an option of this version: seven from the
/// lowest to the highest level, each the same ratio from the last.
pub fn glide_levels(levels: &[f32; 3]) -> [f32; 7] {
    let (low, high) = (levels[0], levels[2]);
    std::array::from_fn(|k| low * (high / low).powf(k as f32 / 6.))
}

/// One frame of the glide of «Zoom fluido»: about a tenth of a second to
/// `to`, never past it.
pub fn glide_step(scale: f32, to: f32, dt: f32) -> f32 {
    let next = to + (scale - to) * (-dt * 25.).exp();
    if (next - to).abs() <= to * 1e-3 {
        to
    } else {
        next
    }
}

/// Next zoom level from the current magnification, towards `direction`.
pub fn zoom_step(levels: &[f32], magnification: f32, direction: i32) -> f32 {
    const EPS: f32 = 1e-3;
    let (low, high) = (levels[0], levels[levels.len() - 1]);
    if direction > 0 {
        levels
            .iter()
            .copied()
            .find(|&z| z > magnification + EPS)
            .unwrap_or(high)
    } else if direction < 0 {
        levels
            .iter()
            .rev()
            .copied()
            .find(|&z| z < magnification - EPS)
            .unwrap_or(low)
    } else {
        magnification.clamp(low, high)
    }
}

/// Map camera: centre in map pixels and orthographic scale (map pixels per
/// logical pixel). The camera transform is derived from it every frame.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub center: Vec2,
    pub scale: f32,
    /// Where the map window lies on the screen.
    pub frame: retzar::hud::MapFrame,
}
impl Default for MapView {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            scale: 1.,
            frame: Default::default(),
        }
    }
}
impl MapView {
    pub fn magnification(&self, surface: Surface) -> f32 {
        surface.dpi / self.scale
    }
    pub fn set_magnification(&mut self, surface: Surface, magnification: f32) {
        self.scale = (surface.dpi / magnification).clamp(0.02, 32.);
    }
    /// Map position under a logical cursor position.
    pub fn world(&self, surface: Surface, cursor: Vec2) -> Vec2 {
        let logical = surface.physical / surface.dpi;
        Vec2::new(
            self.center.x + (cursor.x - logical.x / 2.) * self.scale,
            self.center.y + (logical.y / 2. - cursor.y) * self.scale,
        )
    }
    /// Change scale keeping the map point under `cursor` in place.
    pub fn zoom_at(&mut self, surface: Surface, cursor: Option<Vec2>, scale: f32) {
        if let Some(c) = cursor {
            let before = self.world(surface, c);
            self.scale = scale;
            self.center += before - self.world(surface, c);
        } else {
            self.scale = scale;
        }
    }
    /// Rendered camera position. At whole (or whole-fraction) zoom steps the
    /// view is aligned to physical pixels, so panning never shimmers; the
    /// stored centre keeps sub-pixel motion for slow scrolling.
    pub fn snapped(&self, surface: Surface) -> Vec2 {
        let m = self.magnification(surface);
        let aligned = (m - m.round()).abs() < 1e-3 || (1. / m - (1. / m).round()).abs() < 1e-3;
        if !aligned {
            return self.center;
        }
        let half = surface.physical / 2.;
        let snap = |c: f32, half: f32| ((c * m - half).round() + half) / m;
        Vec2::new(snap(self.center.x, half.x), snap(self.center.y, half.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn surface(w: f32, h: f32, dpi: f32) -> Surface {
        Surface {
            physical: Vec2::new(w, h),
            dpi,
        }
    }
    #[test]
    fn crisp_interface_uses_whole_factors_and_fill_mode_fills() {
        let crisp = Settings::default();
        let fill = Settings {
            ui_mode: 1,
            ..Settings::default()
        };
        for (w, h, factor) in [
            (2560., 1440., 2.),
            (1920., 1080., 1.),
            (3840., 2160., 3.),
            (1600., 1200., 2.),
        ] {
            let s = surface(w, h, 1.);
            let hud = hud_layout(s, &crisp);
            assert_eq!(hud.factor, factor);
            assert!(hud.crisp && hud.size.x >= 800. && hud.size.y >= 600.);
            assert!(hud.size.x * factor >= w && (hud.size.x - 1.) * factor < w);
            let menu = menu_layout(s, &crisp);
            assert_eq!(menu.factor, factor);
            assert_eq!(menu.origin, menu.origin.round());
            assert!(menu.origin.x >= 0. && menu.origin.y >= 0.);
        }
        let hud = hud_layout(surface(2560., 1440., 1.), &fill);
        assert!(!hud.crisp && (hud.factor - 2.4).abs() < 1e-4);
        // Below 800×600 crisp mode falls back to shrinking.
        assert!(!hud_layout(surface(640., 480., 1.), &crisp).crisp);
    }
    #[test]
    fn interface_points_round_trip_with_windows_dpi() {
        for dpi in [1., 1.25, 1.5, 2.] {
            let s = surface(2560., 1440., dpi);
            for layout in [
                hud_layout(s, &Settings::default()),
                menu_layout(s, &Settings::default()),
            ] {
                let target = Vec2::new(387.5, 289.25);
                let cursor = (layout.origin + target * layout.factor) / dpi;
                assert!((layout.point(s, cursor) - target).length() < 1e-3);
            }
        }
    }
    #[test]
    fn sprites_are_placed_on_whole_physical_pixels() {
        let s = surface(2559., 1439., 1.);
        let menu = menu_layout(s, &Settings::default());
        let centre = menu.place(s, Vec2::ZERO, DESIGN);
        // Back to physical: the canvas's left edge must be a whole pixel.
        let left = (centre.x - DESIGN.x / 2.) * menu.factor + s.physical.x / 2.;
        assert!((left - left.round()).abs() < 1e-3, "{left}");
        let hud = hud_layout(s, &Settings::default());
        let c = hud.place(s, Vec2::ZERO, hud.size);
        assert!(((c.x - hud.size.x / 2.) * hud.factor + s.physical.x / 2.).abs() < 1e-3);
    }
    #[test]
    fn the_smooth_zoom_glides_through_seven_even_steps() {
        let g = glide_levels(&[1., 1.5, 2.]);
        assert_eq!((g[0], g[6]), (1., 2.));
        for k in 1..7 {
            assert!((g[k] / g[k - 1] - 2f32.powf(1. / 6.)).abs() < 1e-5);
        }
        let (mut scale, to) = (1., 0.5);
        let mut frames = 0;
        while scale != to {
            let next = glide_step(scale, to, 1. / 60.);
            assert!(next < scale && next >= to);
            scale = next;
            frames += 1;
        }
        assert!((5..=20).contains(&frames), "{frames} frames");
    }
    #[test]
    fn zoom_steps_and_pixel_snapping() {
        let s = surface(2559., 1439., 1.25);
        let hud = hud_layout(s, &Settings::default());
        assert_eq!(default_zoom(&hud), 2.);
        // The three levels chosen by the player, stopping at both ends.
        let levels = zoom_levels(&hud);
        assert_eq!(levels, [1., 1.5, 2.]);
        assert_eq!(opening_zoom(&hud), 2.);
        assert_eq!(zoom_step(&levels, 1.5, 1), 2.);
        assert_eq!(zoom_step(&levels, 2., 1), 2.);
        assert_eq!(zoom_step(&levels, 1.5, -1), 1.);
        assert_eq!(zoom_step(&levels, 1., -1), 1.);
        assert_eq!(zoom_step(&levels, 1.2, 1), 1.5);
        assert_eq!(zoom_step(&levels, 8., 0), 2.);
        assert_eq!(
            zoom_levels(&hud_layout(surface(1920., 1080., 1.), &Settings::default())),
            [0.5, 0.75, 1.]
        );
        let mut view = MapView::default();
        view.set_magnification(s, 2.);
        view.center = Vec2::new(1000.123, -777.777);
        let snapped = view.snapped(s);
        // A map pixel edge (integer coordinate) lands on a physical pixel edge.
        let m = view.magnification(s);
        let x = (0. - snapped.x) * m + s.physical.x / 2.;
        let y = (snapped.y - 0.) * m + s.physical.y / 2.;
        assert!((x - x.round()).abs() < 1e-2 && (y - y.round()).abs() < 1e-2);
        assert!((snapped - view.center).length() <= 1. / m);
        // Zooming at the cursor keeps the map point under it.
        let cursor = Vec2::new(300., 200.);
        let before = view.world(s, cursor);
        view.zoom_at(s, Some(cursor), view.scale / 2.);
        assert!((view.world(s, cursor) - before).length() < 1e-3);
    }
}
