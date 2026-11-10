use chrono::Utc;
use gpui::{
    div, point, prelude::*, px, size, svg, AnyElement, Bounds, Context, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Stateful, Window,
};

use crate::model::deadline_label;
use crate::settings::{Edge, SortMode};
use crate::ui::deadline_editor::{DeadlineState, DeadlineTarget};
use crate::ui::glass::{self, Palette, Radii};
use crate::ui::root::*;
use crate::ui::task_row::deadline_chip;

pub fn icon(name: &str, size_px: f32, color: Hsla) -> gpui::Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size_px)).flex_none().text_color(color)
}

impl Root {
    /// Small square icon button; swallows mouse-down so parents don't start drags.
    pub fn icon_button(
        &self,
        id: impl AsRef<str>,
        name: &str,
        box_px: f32,
        icon_px: f32,
        pal: Palette,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> Stateful<gpui::Div> {
        let hover = pal.hover;
        let id: gpui::SharedString = id.as_ref().to_string().into();
        let sel = id.to_string();
        div()
            .id(id)
            .debug_selector(move || sel)
            .size(px(box_px))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _e, window, cx| {
                f(this, window, cx);
            }))
            .child(icon(name, icon_px, pal.secondary))
    }

    // MARK: mouse handling on the canvas

    pub fn canvas_mouse_move(&mut self, e: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else { return };
        self.touch();
        match drag {
            Drag::Move { start_mouse, start_origin } => {
                let mut r = self.rect;
                r.origin = start_origin + (e.position - start_mouse);
                self.rect = clamp_rect(r, self.canvas);
                self.expanded = self.rect;
            }
            Drag::Resize { start_mouse, start_size } => {
                let d = e.position - start_mouse;
                let max_w = self.canvas.width - self.rect.origin.x;
                let max_h = self.canvas.height - self.rect.origin.y;
                self.rect.size = size(
                    (start_size.width + d.x).max(MIN_SIZE.width).min(max_w),
                    (start_size.height + d.y).max(MIN_SIZE.height).min(max_h),
                );
                self.expanded = self.rect;
            }
            Drag::Tab { start_mouse, start_origin, moved } => {
                let d = e.position - start_mouse;
                let dist = (f32::from(d.x).powi(2) + f32::from(d.y).powi(2)).sqrt();
                if !moved && dist < 3.0 {
                    return;
                }
                self.drag = Some(Drag::Tab { start_mouse, start_origin, moved: true });
                self.anim = None;
                self.rect.origin = start_origin + d;
                self.rect = clamp_rect(self.rect, self.canvas);
            }
            Drag::Reorder { id, start_mouse, active } => {
                if !active {
                    let d = e.position - start_mouse;
                    let dist = (f32::from(d.x).powi(2) + f32::from(d.y).powi(2)).sqrt();
                    if dist < 4.0 {
                        return;
                    }
                    self.drag = Some(Drag::Reorder { id, start_mouse, active: true });
                    self.reorder_active = Some(id);
                    self.store.begin_drag();
                }
            }
        }
        cx.notify();
    }

    pub fn canvas_mouse_up(&mut self, _e: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        match drag {
            Drag::Move { .. } | Drag::Resize { .. } => self.persist(cx),
            Drag::Tab { moved, .. } => {
                if moved {
                    self.snap_tab(cx);
                    self.persist(cx);
                } else {
                    self.expand(window, cx);
                }
            }
            Drag::Reorder { .. } => self.reorder_active = None,
        }
        cx.notify();
    }

    pub fn header_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.click_count == 2 {
            self.collapse(window, cx);
            return;
        }
        window.focus(&self.focus.clone(), cx);
        if self.mode == CanvasMode::Toplevel {
            window.start_window_move();
        } else {
            self.anim = None;
            self.drag = Some(Drag::Move { start_mouse: e.position, start_origin: self.rect.origin });
        }
        cx.notify();
    }

    fn grip_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.mode == CanvasMode::Toplevel {
            window.start_window_resize(gpui::ResizeEdge::BottomRight);
        } else {
            self.anim = None;
            self.drag = Some(Drag::Resize { start_mouse: e.position, start_size: self.rect.size });
        }
    }

    // MARK: pieces

    fn radii(&self) -> Radii {
        let r = if self.collapsed { glass::TAB_RADIUS } else { glass::panel_radius(self.settings.window_style) };
        let (flat_l, flat_r) = (
            self.collapsed && self.edge == Edge::Left,
            self.collapsed && self.edge == Edge::Right,
        );
        let z = px(0.);
        let r = px(r);
        (
            if flat_l { z } else { r },
            if flat_r { z } else { r },
            if flat_r { z } else { r },
            if flat_l { z } else { r },
        )
    }

    fn add_field(&mut self, pal: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.new_deadline;
        let now = Utc::now();
        let deadline_btn = div()
            .id("new-deadline")
            .debug_selector(|| "new-deadline".into())
            .flex_none()
            .cursor_pointer()
            .on_click(cx.listener(|this, _e, _w, cx| {
                let state = match this.new_deadline {
                    Some(p) => DeadlineState::from_existing(p.date, p.has_time, p.repeat, true, DeadlineTarget::New),
                    None => DeadlineState::fresh(DeadlineTarget::New),
                };
                this.overlay = Some(Overlay::Deadline(state));
                cx.notify();
            }))
            .child(match pending {
                Some(p) => deadline_chip(p.date, p.has_time, p.repeat.is_some(), false, now, pal).into_any_element(),
                None => icon("calendar-plus", 14.0, pal.secondary).into_any_element(),
            });
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .py(px(9.))
            .child(div().w(px(15.)).flex().justify_center().child(icon("plus", 12.0, pal.secondary)))
            .child(div().flex_1().text_size(px(13.)).text_color(pal.fg).child(self.add_input.clone()))
            .child(deadline_btn)
    }

    fn done_header(&mut self, count: usize, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let show = self.settings.show_done;
        div()
            .id("done-header")
            .flex()
            .items_center()
            .gap(px(5.))
            .px(px(10.))
            .pt(px(10.))
            .pb(px(4.))
            .cursor_pointer()
            .text_color(pal.secondary)
            .on_click(cx.listener(|this, _e, _w, cx| {
                this.settings.show_done = !this.settings.show_done;
                this.save_settings(cx);
            }))
            .child(icon(if show { "chevron-down" } else { "chevron-right" }, 10.0, pal.secondary))
            .child(div().text_size(px(11.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Done"))
            .child(div().text_size(px(11.)).child(count.to_string()))
            .into_any_element()
    }

    fn task_list(&mut self, pal: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.store.items().is_empty() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(pal.tertiary)
                .child("Nothing to do.")
                .into_any_element();
        }
        let now = Utc::now();
        self.dbg_row = 0;
        let list_alpha = self.list_fade.map_or(1.0, |t| crate::anim::since_progress(t, std::time::Duration::from_millis(150)));
        let mut rows: Vec<AnyElement> = vec![];
        for item in self.open_items() {
            rows.push(self.task_row(item, now, pal, window, cx));
        }
        let done = self.done_items();
        if !done.is_empty() {
            rows.push(self.done_header(done.len(), pal, cx));
            if self.settings.show_done {
                for item in done {
                    rows.push(self.task_row(item, now, pal, window, cx));
                }
            }
        }
        div()
            .id("task-list")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .opacity(list_alpha)
            .child(div().flex().flex_col().gap(px(1.)).px(px(6.)).py(px(5.)).children(rows))
            .into_any_element()
    }

    fn footer(&mut self, pal: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.store.open_count();
        let done = self.store.done_count();
        let sorted = if self.settings.sort_mode == SortMode::Deadline { " · by deadline" } else { "" };
        let summary = match (open, done) {
            (0, 0) => "No tasks".to_string(),
            (0, _) => "All done".to_string(),
            (o, _) => format!("{o} left{sorted}"),
        };
        let grip = div()
            .id("grip")
            .size(px(14.))
            .flex_none()
            .cursor(gpui::CursorStyle::ResizeUpLeftDownRight)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::grip_mouse_down))
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        let mut builder = gpui::PathBuilder::stroke(px(1.));
                        for off in [4.0f32, 8.0] {
                            builder.move_to(point(b.right() - px(off), b.bottom() - px(1.)));
                            builder.line_to(point(b.right() - px(1.), b.bottom() - px(off)));
                        }
                        let mut c = pal.secondary;
                        c.a *= 0.9;
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, c);
                        }
                    },
                )
                .size_full(),
            );
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(6.))
            .py(px(6.))
            .child(div().text_size(px(11.)).text_color(pal.secondary).child(summary))
            .child(div().flex_1())
            .when(self.current_list_linked() && !self.google_status.is_empty(), |d| {
                d.child(div().text_size(px(10.)).text_color(pal.tertiary).truncate().child(format!("Google · {}", self.google_status)))
            })
            .when(done > 0, |d| {
                d.child(
                    div()
                        .id("clear-done")
                        .text_size(px(11.))
                        .text_color(pal.secondary)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _e, _w, cx| this.edit_store(cx, |s| s.clear_completed())))
                        .child("Clear done"),
                )
            })
            .child(grip)
    }

    fn expanded_content(&mut self, pal: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let width = self.rect.size.width.max(MIN_SIZE.width);
        let height = self.rect.size.height.max(MIN_SIZE.height);
        let header = self.header(pal, window, cx);
        let add = self.add_field(pal, cx);
        let list = self.task_list(pal, window, cx);
        let footer = self.footer(pal, cx);
        let mut c = div()
            .absolute()
            .top_0()
            .w(width)
            .h(height)
            .flex()
            .flex_col()
            .child(header)
            .child(add)
            .child(div().h(px(1.)).mx(px(0.)).bg(pal.divider))
            .child(list)
            .child(footer);
        c = if self.edge == Edge::Right && self.collapsed { c.right_0() } else { c.left_0() };
        c.into_any_element()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frame_update(window, cx);
        let pal = self.palette(window);
        let alpha = self.alpha.value();
        let style = self.settings.window_style;
        let radii = self.radii();
        let rect = self.rect;
        let show_tab = self.collapsed && self.anim.is_none();

        let inner = if show_tab {
            self.tab_content(pal, cx)
        } else {
            self.expanded_content(pal, window, cx)
        };

        let mut panel = div()
            .id("panel")
            .absolute()
            .left(rect.origin.x)
            .top(rect.origin.y)
            .w(rect.size.width)
            .h(rect.size.height)
            .overflow_hidden()
            .rounded_tl(radii.0)
            .rounded_tr(radii.1)
            .rounded_br(radii.2)
            .rounded_bl(radii.3)
            .bg(glass::base_fill(style, pal.dark))
            .opacity(alpha)
            .shadow_lg()
            .text_color(pal.fg)
            .text_size(px(13.))
            .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                this.mouse_inside = *hovered;
                this.touch();
                cx.notify();
            }))
            .child(glass::glass_overlays(style, pal.dark, radii))
            .child(inner);
        if self.mode == CanvasMode::Toplevel {
            panel = panel.left_0().top_0();
        }

        let mut canvas = div()
            .id("canvas")
            .size_full()
            .relative()
            .key_context("Panel")
            .track_focus(&self.focus)
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| this.canvas_mouse_move(e, w, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, w, cx| this.canvas_mouse_up(e, w, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, w, cx| this.canvas_mouse_up(e, w, cx)))
            .on_action(cx.listener(|this, _: &CollapseKey, w, cx| {
                if this.overlay.is_some() {
                    this.overlay = None;
                    cx.notify();
                } else {
                    this.collapse(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SelUp, w, cx| this.move_selection(-1, w, cx)))
            .on_action(cx.listener(|this, _: &SelDown, w, cx| this.move_selection(1, w, cx)))
            .on_action(cx.listener(|this, _: &ToggleSel, _w, cx| {
                if let Some(id) = this.selected {
                    this.toggle_task(id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditSel, w, cx| {
                if let Some(id) = this.selected {
                    this.begin_editing(id, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DeleteSel, _w, cx| {
                if let Some(id) = this.selected {
                    this.delete_task(id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _w, cx| this.move_selected_task(-1, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _w, cx| this.move_selected_task(1, cx)))
            .on_action(cx.listener(|this, _: &NewTask, w, cx| {
                this.expand(w, cx);
                this.focus_add(w, cx);
            }))
            .on_action(cx.listener(|this, _: &UndoKey, w, cx| this.undo(w, cx)))
            .on_action(cx.listener(|this, _: &RedoKey, w, cx| this.redo(w, cx)))
            .on_action(cx.listener(|_this, _: &QuitKey, _w, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &NewWorkspaceKey, w, cx| {
                this.expand(w, cx);
                this.new_workspace(w, cx);
            }));
        macro_rules! ws_action {
            ($($ty:ident => $i:expr),*) => {
                $(canvas = canvas.on_action(cx.listener(|this, _: &$ty, w, cx| this.switch_workspace_at($i, w, cx)));)*
            };
        }
        ws_action!(Ws1 => 0, Ws2 => 1, Ws3 => 2, Ws4 => 3, Ws5 => 4, Ws6 => 5, Ws7 => 6, Ws8 => 7, Ws9 => 8);

        canvas = canvas.child(panel);

        // Drag preview while reordering.
        if let Some(id) = self.reorder_active {
            if let Some(item) = self.store.item(id) {
                let m = window.mouse_position();
                canvas = canvas.child(
                    div()
                        .absolute()
                        .left(m.x + px(10.))
                        .top(m.y - px(10.))
                        .w(px(230.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(7.))
                        .bg(pal.menu_bg)
                        .shadow_md()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .text_color(pal.fg)
                        .text_size(px(13.))
                        .child(div().size(px(15.)).rounded_full().border_1().border_color(pal.secondary))
                        .child(div().truncate().child(item.title.clone())),
                );
            }
        }

        if self.overlay.is_some() {
            canvas = canvas.child(self.overlay_layer(pal, window, cx));
        }
        let _ = deadline_label;
        let _: Option<Bounds<Pixels>> = None;
        canvas
    }
}
