use chrono::Utc;
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, MouseButton, MouseDownEvent};

use crate::settings::Edge;
use crate::ui::glass::Palette;
use crate::ui::panel::icon;
use crate::ui::root::*;

impl Root {
    /// The slim edge tab: click expands, drag moves (re-snaps to the nearest edge on release).
    pub fn tab_content(&mut self, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.store.open_count();
        let overdue = self.store.overdue_count(Utc::now());
        let letter = (self.store.workspaces.len() > 1)
            .then(|| self.store.current_workspace().name.chars().next().map(|c| c.to_uppercase().to_string()))
            .flatten();
        let chevron = if self.edge == Edge::Right { "chevron-left" } else { "chevron-right" };
        div()
            .id("tab")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _w, cx| {
                this.anim = None;
                this.drag = Some(Drag::Tab { start_mouse: e.position, start_origin: this.rect.origin, moved: false });
                cx.notify();
            }))
            .child(icon(chevron, 14.0, pal.secondary))
            .when_some(letter, |d, l| {
                d.child(div().text_size(px(10.)).font_weight(FontWeight::BOLD).text_color(pal.secondary).child(l))
            })
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(open.to_string()))
            .when(overdue > 0, |d| {
                d.child(
                    div()
                        .min_w(px(16.))
                        .h(px(14.))
                        .px(px(4.))
                        .rounded_full()
                        .bg(pal.red)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(gpui::white())
                        .child(overdue.to_string()),
                )
            })
            .into_any_element()
    }
}
