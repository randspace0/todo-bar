use gpui::{div, linear_color_stop, linear_gradient, prelude::*, px, Hsla, Pixels, Rgba};

use crate::settings::WindowStyle;

pub fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }.into()
}

pub fn white(a: f32) -> Hsla {
    rgba8(255, 255, 255, a)
}

pub fn black(a: f32) -> Hsla {
    rgba8(0, 0, 0, a)
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub fg: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub accent: Hsla,
    pub red: Hsla,
    pub orange: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub divider: Hsla,
    pub menu_bg: Hsla,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        let fg = if dark { white(1.0) } else { rgba8(28, 28, 30, 1.0) };
        let mut secondary = fg;
        secondary.a = 0.6;
        let mut tertiary = fg;
        tertiary.a = 0.35;
        let mut hover = fg;
        hover.a = 0.07;
        let mut divider = fg;
        divider.a = 0.12;
        let accent = if dark { rgba8(10, 132, 255, 1.0) } else { rgba8(0, 122, 255, 1.0) };
        let mut selected = accent;
        selected.a = 0.18;
        Self {
            dark,
            fg,
            secondary,
            tertiary,
            accent,
            red: rgba8(255, 69, 58, 1.0),
            orange: rgba8(255, 159, 10, 1.0),
            hover,
            selected,
            divider,
            menu_bg: if dark { rgba8(44, 44, 48, 0.98) } else { rgba8(250, 250, 252, 0.98) },
        }
    }
}

pub const PANEL_RADIUS_GLASS: f32 = 18.0;
pub const PANEL_RADIUS_STANDARD: f32 = 12.0;
pub const TAB_RADIUS: f32 = 9.0;

pub fn panel_radius(style: WindowStyle) -> f32 {
    match style {
        WindowStyle::Glass => PANEL_RADIUS_GLASS,
        WindowStyle::Standard => PANEL_RADIUS_STANDARD,
    }
}

/// Base fill colour of the panel for the style/theme. Higher alpha than the macOS fallback (0.72):
/// there is no compositor blur behind the panel here, so a thin fill hurts legibility.
pub fn base_fill(style: WindowStyle, dark: bool) -> Hsla {
    match (style, dark) {
        (WindowStyle::Glass, true) => rgba8(30, 30, 34, 0.88),
        (WindowStyle::Glass, false) => rgba8(246, 246, 248, 0.90),
        (WindowStyle::Standard, true) => rgba8(30, 30, 34, 0.96),
        (WindowStyle::Standard, false) => rgba8(246, 246, 248, 0.96),
    }
}

/// Corner radii: (top-left, top-right, bottom-right, bottom-left). Tab is flat on the screen-edge side.
pub type Radii = (Pixels, Pixels, Pixels, Pixels);

/// Absolutely-positioned overlays (sheen + rim) that make the panel read as glass. gpui can't draw
/// gradient borders, so the rim is a uniform border plus a brighter top/left highlight.
pub fn glass_overlays(style: WindowStyle, dark: bool, r: Radii) -> impl IntoElement {
    let (tl, tr, br, bl) = r;
    let rounded = |d: gpui::Div| d.rounded_tl(tl).rounded_tr(tr).rounded_br(br).rounded_bl(bl);
    let mut holder = div().absolute().top_0().left_0().size_full();
    match style {
        WindowStyle::Glass => {
            let sheen_a = if dark { 0.04 } else { 0.22 };
            let top_a = if dark { 0.12 } else { 0.35 };
            let rim_lo = if dark { 0.20 } else { 0.60 };
            let rim_hi = if dark { 0.38 } else { 0.90 };
            holder = holder
                .child(rounded(div().absolute().top_0().left_0().size_full().bg(white(sheen_a))))
                .child(rounded(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(gpui::relative(0.35))
                        .bg(linear_gradient(
                            180.0,
                            linear_color_stop(white(top_a), 0.0),
                            linear_color_stop(white(0.0), 1.0),
                        )),
                ))
                .child(rounded(div().absolute().top_0().left_0().size_full().border_1().border_color(white(rim_lo))))
                .child(rounded(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .border_t_1()
                        .border_l_1()
                        .border_color(white(rim_hi)),
                ));
            if !dark {
                holder = holder.child(rounded(
                    div().absolute().top(px(-1.0)).left(px(-1.0)).right(px(-1.0)).bottom(px(-1.0)).border_1().border_color(black(0.07)),
                ));
            }
        }
        WindowStyle::Standard => {
            let mut c = if dark { white(1.0) } else { rgba8(28, 28, 30, 1.0) };
            c.a = 0.1;
            holder = holder.child(rounded(div().absolute().top_0().left_0().size_full().border_1().border_color(c)));
        }
    }
    holder
}
