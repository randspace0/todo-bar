use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui::{div, prelude::*, px, relative, AnyElement, Context, MouseButton, MouseDownEvent, Window};
use uuid::Uuid;

use crate::anim::since_progress;
use crate::model::{deadline_label, TodoItem};
use crate::ui::glass::Palette;
use crate::ui::panel::icon;
use crate::ui::root::*;

pub fn deadline_chip(
    date: DateTime<Utc>,
    has_time: bool,
    repeats: bool,
    is_done: bool,
    now: DateTime<Utc>,
    pal: Palette,
) -> impl IntoElement {
    let mut probe = TodoItem::new(String::new());
    probe.deadline = Some(date);
    probe.deadline_has_time = has_time;
    probe.is_done = is_done;
    let overdue = probe.is_overdue(now);
    let tint = if overdue {
        pal.red
    } else if probe.is_due_today(now) {
        pal.orange
    } else {
        pal.secondary
    };
    div()
        .flex()
        .items_center()
        .gap(px(3.))
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(tint)
        .whitespace_nowrap()
        .child(icon(if overdue { "alert-circle" } else { "calendar" }, 11.0, tint))
        .child(deadline_label(date, has_time, now))
        .when(repeats, |d| d.child(icon("repeat", 10.0, tint)))
}

fn is_link(token: &str) -> bool {
    token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.")
}

impl Root {
    fn notes_view(&mut self, id: Uuid, notes: &str, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.expanded_notes.contains(&id);
        let long = notes.contains('\n') || notes.chars().count() > 36;
        let mut lines = div().flex().flex_col().flex_1().min_w(px(0.));
        if !expanded {
            lines = lines.max_h(px(15.)).overflow_hidden();
        }
        // Collapsed: first line only, no wrapping (clipped).
        for (li, line) in notes.split('\n').enumerate().take(if expanded { usize::MAX } else { 1 }) {
            let mut row = div().flex().gap_x(px(4.));
            row = if expanded { row.flex_wrap() } else { row.whitespace_nowrap().overflow_hidden() };
            for (ti, token) in line.split_whitespace().enumerate() {
                if is_link(token) {
                    let url = if token.starts_with("www.") { format!("https://{token}") } else { token.to_string() };
                    row = row.child(
                        div()
                            .id(format!("link-{id}-{li}-{ti}"))
                            .text_color(pal.accent)
                            .underline()
                            .cursor_pointer()
                            .on_click(move |_e, _w, _cx| {
                                let _ = open::that(&url);
                            })
                            .child(token.to_string()),
                    );
                } else {
                    row = row.child(token.to_string());
                }
            }
            lines = lines.child(row.min_h(px(15.)));
        }
        div()
            .flex()
            .items_start()
            .gap(px(4.))
            .text_size(px(11.))
            .text_color(pal.secondary)
            .child(lines)
            .when(long, |d| {
                d.child(
                    div()
                        .id(format!("notes-toggle-{id}"))
                        .w(px(14.))
                        .h(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _e, _w, cx| {
                            if !this.expanded_notes.remove(&id) {
                                this.expanded_notes.insert(id);
                            }
                            cx.notify();
                        }))
                        .child(icon(if expanded { "chevron-up" } else { "chevron-down" }, 9.0, pal.tertiary)),
                )
            })
            .into_any_element()
    }

    pub fn task_row(
        &mut self,
        item: TodoItem,
        now: DateTime<Utc>,
        pal: Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = item.id;
        if self.known_rows.insert(id) {
            self.appear.insert(id, std::time::Instant::now());
        }
        let appear = self.appear.get(&id).map_or(1.0, |t| since_progress(*t, Duration::from_millis(220)));
        let dbg_name = format!("row-{}", self.dbg_row);
        self.dbg_row += 1;
        let editing = self.editing == Some(id);
        let selected = self.selected == Some(id);
        let dragging = self.reorder_active == Some(id);
        let v = match self.check_anim.get(&id) {
            Some((to, start)) => {
                let p = since_progress(*start, Duration::from_millis(250));
                if *to {
                    p
                } else {
                    1.0 - p
                }
            }
            None => {
                if item.is_done {
                    1.0
                } else {
                    0.0
                }
            }
        };
        let group = format!("row-{id}");

        // check button
        let ring = if v > 0.5 { pal.accent } else { pal.secondary };
        let check = div()
            .id(format!("chk-{id}"))
            .mt(px(1.))
            .size(px(15.))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(ring)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _e, _w, cx| this.toggle_task(id, cx)))
            .child(
                div()
                    .size(px(15. * (0.3 + 0.7 * v)))
                    .rounded_full()
                    .bg(pal.accent)
                    .opacity(v)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("check", 9.0 * (0.5 + 0.5 * v), gpui::white())),
            );

        // title with left-to-right strike-through
        let title_text = || div().text_size(px(13.)).child(item.title.clone());
        let title: AnyElement = if v >= 0.999 {
            title_text().line_through().into_any_element()
        } else if v > 0.001 {
            div()
                .relative()
                .child(title_text())
                .child(
                    div().absolute().top_0().left_0().h_full().w(relative(v)).overflow_hidden().child(
                        div()
                            .text_size(px(13.))
                            .whitespace_nowrap()
                            .text_color(gpui::transparent_black())
                            .line_through()
                            .text_decoration_color(pal.fg)
                            .child(item.title.clone()),
                    ),
                )
                .into_any_element()
        } else {
            title_text().into_any_element()
        };

        let mut column = div().flex().flex_col().gap(px(3.)).flex_1().min_w(px(0.)).opacity(1.0 - 0.55 * v);
        if editing {
            column = column
                .child(div().text_size(px(13.)).text_color(pal.fg).child(self.title_input.clone()))
                .child(div().text_size(px(11.)).text_color(pal.secondary).child(self.notes_input.clone()));
        } else {
            column = column.child(title);
            if let Some(notes) = item.notes.as_deref() {
                column = column.child(self.notes_view(id, notes, pal, cx));
            }
        }
        if let Some(deadline) = item.deadline {
            column = column.child(
                div()
                    .id(format!("dl-{id}"))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _e, _w, cx| this.open_task_deadline(id, cx)))
                    .child(deadline_chip(deadline, item.deadline_has_time, item.repeat_rule.is_some(), item.is_done, now, pal)),
            );
        }

        let actions = div()
            .flex()
            .flex_none()
            .when(!selected || editing, |d| d.opacity(0.0).group_hover(group.clone(), |s| s.opacity(1.0)))
            .when(editing, |d| d.invisible())
            .when(item.deadline.is_none(), |d| {
                d.child(self.icon_button(format!("adddl-{id}"), "calendar-plus", 18.0, 11.0, pal, cx, move |this, _w, cx| {
                    this.open_task_deadline(id, cx)
                }))
            })
            .child(self.icon_button(format!("del-{id}"), "x", 18.0, 10.0, pal, cx, move |this, _w, cx| this.delete_task(id, cx)));

        let hover_bg = pal.hover;
        let can_reorder = self.can_reorder(&item);
        div()
            .id(format!("row-{id}"))
            .group(group)
            .flex()
            .items_start()
            .gap(px(9.))
            .py(px(6.))
            .px(px(8.))
            .rounded(px(7.))
            .when(selected, |d| d.bg(pal.selected))
            .when(!selected, |d| d.hover(move |s| s.bg(hover_bg)))
            .debug_selector(move || dbg_name)
            .opacity(appear * if dragging { 0.4 } else { 1.0 })
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                if this.editing == Some(id) {
                    return;
                }
                if e.click_count >= 2 {
                    this.begin_editing(id, window, cx);
                    return;
                }
                this.selected = Some(id);
                window.focus(&this.focus.clone(), cx);
                if can_reorder {
                    this.drag = Some(Drag::Reorder { id, start_mouse: e.position, active: false });
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _w, cx| {
                this.selected = Some(id);
                this.overlay = Some(Overlay::TaskMenu { id, pos: e.position });
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, _e, _w, cx| {
                if let Some(rid) = this.reorder_active {
                    if rid != id && can_reorder {
                        this.edit_store(cx, |s| s.move_during_drag(rid, id));
                    }
                }
            }))
            .child(check)
            .child(column)
            .child(actions)
            .into_any_element()
    }
}
