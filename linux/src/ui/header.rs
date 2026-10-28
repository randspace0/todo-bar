use gpui::{
    div, point, prelude::*, px, AnyElement, Context, FontWeight, MouseButton, Pixels, Point, SharedString, Stateful,
    Window,
};

use crate::settings::{SortMode, Theme, WindowStyle};
use crate::ui::deadline_editor::{DeadlineState, DeadlineTarget};
use crate::ui::glass::Palette;
use crate::ui::panel::icon;
use crate::ui::root::*;

pub fn menu_box(pal: Palette) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(180.))
        .p(px(4.))
        .rounded(px(9.))
        .bg(pal.menu_bg)
        .border_1()
        .border_color(pal.divider)
        .shadow_lg()
        .text_size(px(12.5))
        .text_color(pal.fg)
        .occlude()
}

pub fn menu_label(text: &str, pal: Palette) -> gpui::Div {
    div()
        .px(px(8.))
        .pt(px(6.))
        .pb(px(2.))
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(pal.tertiary)
        .child(text.to_string())
}

pub fn menu_divider(pal: Palette) -> gpui::Div {
    div().h(px(1.)).my(px(4.)).bg(pal.divider)
}

impl Root {
    pub fn menu_item(
        &self,
        id: impl AsRef<str>,
        label: impl Into<SharedString>,
        hint: Option<String>,
        checked: bool,
        enabled: bool,
        pal: Palette,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> Stateful<gpui::Div> {
        let hover = pal.selected;
        let id: SharedString = id.as_ref().to_string().into();
        let sel = id.to_string();
        let mut item = div()
            .id(id)
            .debug_selector(move || sel)
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(5.))
            .child(div().w(px(12.)).flex_none().when(checked, |d| d.child(icon("check", 11.0, pal.fg))))
            .child(div().flex_1().whitespace_nowrap().child(label.into()))
            .when_some(hint, |d, h| d.child(div().text_color(pal.tertiary).text_size(px(11.)).child(h)));
        if enabled {
            item = item
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _e, window, cx| f(this, window, cx)));
        } else {
            item = item.opacity(0.4);
        }
        item
    }

    pub fn header(&mut self, pal: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let name = self.store.current_workspace().name.clone();
        let open = self.store.open_count();
        let dark = pal.dark;

        let title: AnyElement = if self.renaming_ws {
            div()
                .w(px(170.))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.ws_input.clone())
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id("ws-btn")
                        .debug_selector(|| "ws-btn".into())
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _e, _w, cx| {
                            this.overlay = if matches!(this.overlay, Some(Overlay::Workspaces)) {
                                None
                            } else {
                                Some(Overlay::Workspaces)
                            };
                            cx.notify();
                        }))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(name))
                        .child(icon("chevron-down", 10.0, pal.secondary)),
                )
                .when(open > 0, |d| d.child(div().text_size(px(11.)).text_color(pal.secondary).child(open.to_string())))
                .into_any_element()
        };

        let theme_btn =
            self.icon_button("theme-btn", if dark { "sun" } else { "moon" }, 22.0, 12.0, pal, cx, |this, window, cx| {
                this.toggle_light_dark(window, cx)
            });
        let options_btn = self.icon_button("options-btn", "ellipsis", 24.0, 14.0, pal, cx, |this, _w, cx| {
            this.overlay = if matches!(this.overlay, Some(Overlay::Options)) { None } else { Some(Overlay::Options) };
            cx.notify();
        });
        let collapse_btn = self.icon_button("collapse-btn", "collapse", 26.0, 14.0, pal, cx, |this, w, cx| this.collapse(w, cx));
        let _ = window;

        div()
            .flex()
            .items_center()
            .h(px(26. + 10.))
            .pl(px(14.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(2.))
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .flex_1()
                    .h_full()
                    .child(title)
                    .child(
                        div()
                            .id("header-drag")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::header_mouse_down)),
                    ),
            )
            .child(theme_btn)
            .child(options_btn)
            .child(collapse_btn)
            .into_any_element()
    }

    fn clamp_menu_pos(&self, p: Point<Pixels>, w: f32, h: f32) -> Point<Pixels> {
        let x = f32::from(p.x).min(f32::from(self.canvas.width) - w - 6.0).max(6.0);
        let y = f32::from(p.y).min(f32::from(self.canvas.height) - h - 6.0).max(6.0);
        point(px(x), px(y))
    }

    /// Dismiss layer + whichever menu/popover is open.
    pub fn overlay_layer(&mut self, pal: Palette, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let catcher = div()
            .id("overlay-catcher")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _e, _w, cx| {
                this.overlay = None;
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _e, _w, cx| {
                this.overlay = None;
                cx.notify();
            }));
        let rect = self.rect;
        let body: AnyElement = match self.overlay.take() {
            None => div().into_any_element(),
            Some(Overlay::Workspaces) => {
                self.overlay = Some(Overlay::Workspaces);
                let pos = self.clamp_menu_pos(point(rect.origin.x + px(10.), rect.origin.y + px(38.)), 200.0, 200.0);
                self.workspace_menu(pos, pal, cx)
            }
            Some(Overlay::Options) => {
                self.overlay = Some(Overlay::Options);
                let w = 200.0;
                let x = f32::from(rect.origin.x + rect.size.width) - w - 8.0;
                let pos = self.clamp_menu_pos(point(px(x), rect.origin.y + px(38.)), w, 300.0);
                self.options_menu(pos, pal, cx)
            }
            Some(Overlay::Deadline(state)) => {
                let x = f32::from(rect.origin.x) + (f32::from(rect.size.width) - 236.0) / 2.0;
                let pos = self.clamp_menu_pos(point(px(x), rect.origin.y + px(44.)), 236.0, 360.0);
                self.deadline_popover(state, pos, pal, cx)
            }
            Some(Overlay::TaskMenu { id, pos }) => {
                self.overlay = Some(Overlay::TaskMenu { id, pos });
                let pos = self.clamp_menu_pos(pos, 190.0, 190.0);
                self.task_menu(id, pos, pal, cx)
            }
            Some(Overlay::GoogleLists(lists)) => {
                self.overlay = Some(Overlay::GoogleLists(lists.clone()));
                let pos = self.clamp_menu_pos(point(rect.origin.x + px(10.), rect.origin.y + px(38.)), 220.0, 240.0);
                self.google_lists_menu(lists, pos, pal, cx)
            }
            Some(Overlay::MoveTo { id, pos }) => {
                self.overlay = Some(Overlay::MoveTo { id, pos });
                let pos = self.clamp_menu_pos(pos, 190.0, 120.0);
                self.move_to_menu(id, pos, pal, cx)
            }
            Some(Overlay::ConfirmDeleteWorkspace(id)) => {
                self.overlay = Some(Overlay::ConfirmDeleteWorkspace(id));
                self.confirm_delete(id, pal, cx)
            }
        };
        div().absolute().top_0().left_0().size_full().child(catcher).child(body).into_any_element()
    }

    fn abs_at(pos: Point<Pixels>) -> gpui::Div {
        div().absolute().left(pos.x).top(pos.y)
    }

    fn workspace_menu(&mut self, pos: Point<Pixels>, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let current = self.store.current_workspace_id;
        let name = self.store.current_workspace().name.clone();
        let many = self.store.workspaces.len() > 1;
        let mut menu = menu_box(pal);
        for (i, ws) in self.store.workspaces.clone().into_iter().enumerate() {
            let id = ws.id;
            menu = menu.child(self.menu_item(
                format!("ws-{i}"),
                ws.name,
                (i < 9).then(|| format!("Ctrl+{}", i + 1)),
                ws.id == current,
                true,
                pal,
                cx,
                move |this, w, cx| this.switch_workspace(id, w, cx),
            ));
        }
        if self.google_connected {
            menu = menu.child(menu_divider(pal));
            menu = if self.current_list_linked() {
                menu.child(self.menu_item("ws-gunlink", "Unlink Google Tasks list", None, false, true, pal, cx, |this, _w, cx| this.unlink_list(cx)))
            } else {
                menu.child(self.menu_item("ws-glink", "Link to Google Tasks list…", None, false, true, pal, cx, |this, _w, cx| this.open_google_lists(cx)))
            };
        }
        menu = menu
            .child(menu_divider(pal))
            .child(self.menu_item("ws-new", "New Workspace", Some("Ctrl+Shift+N".into()), false, true, pal, cx, |this, w, cx| {
                this.new_workspace(w, cx)
            }))
            .child(self.menu_item("ws-rename", format!("Rename “{name}”…"), None, false, true, pal, cx, |this, w, cx| {
                this.begin_rename(w, cx)
            }))
            .child(self.menu_item("ws-delete", format!("Delete “{name}”…"), None, false, many, pal, cx, move |this, _w, cx| {
                this.overlay = None;
                this.request_delete_workspace(current, cx);
            }));
        Self::abs_at(pos).child(menu).into_any_element()
    }

    fn options_menu(&mut self, pos: Point<Pixels>, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut menu = menu_box(pal)
            .id("options-menu")
            .max_h(self.canvas.height - pos.y - px(8.))
            .overflow_y_scroll()
            .child(menu_label("Sort", pal));
        for m in SortMode::ALL {
            menu = menu.child(self.menu_item(format!("sort-{m:?}"), m.title(), None, self.settings.sort_mode == m, true, pal, cx, move |this, _w, cx| {
                this.settings.sort_mode = m;
                this.save_settings(cx);
            }));
        }
        menu = menu.child(menu_label("Theme", pal));
        for t in Theme::ALL {
            menu = menu.child(self.menu_item(format!("theme-{t:?}"), t.title(), None, self.settings.theme == t, true, pal, cx, move |this, _w, cx| {
                this.settings.theme = t;
                this.save_settings(cx);
            }));
        }
        menu = menu.child(menu_label("Style", pal));
        for s in WindowStyle::ALL {
            menu = menu.child(self.menu_item(format!("style-{s:?}"), s.title(), None, self.settings.window_style == s, true, pal, cx, move |this, _w, cx| {
                this.set_window_style(s, cx)
            }));
        }
        let launch = crate::autostart::is_enabled();
        let done = self.store.done_count();
        menu = menu.child(menu_divider(pal)).child(menu_label("Google Tasks", pal));
        if self.google_creds.is_none() {
            menu = menu.child(self.menu_item("g-noid", "Set client ID in settings.json", None, false, false, pal, cx, |_t, _w, _c| {}));
        } else if !self.google_connected {
            menu = menu.child(self.menu_item("g-connect", "Connect Google Tasks…", None, false, true, pal, cx, |this, _w, cx| {
                this.overlay = None;
                this.google_connect(cx);
            }));
        } else {
            let busy = self.syncing;
            menu = menu
                .child(self.menu_item("g-sync", "Sync now", None, false, !busy, pal, cx, |this, _w, cx| {
                    this.overlay = None;
                    this.sync_now(cx);
                }))
                .child(self.menu_item("g-disconnect", "Disconnect Google Tasks", None, false, true, pal, cx, |this, _w, cx| {
                    this.overlay = None;
                    this.google_disconnect(cx);
                }));
        }
        if !self.google_status.is_empty() {
            menu = menu.child(div().px(px(8.)).py(px(2.)).max_w(px(190.)).text_size(px(10.5)).text_color(pal.tertiary).child(self.google_status.clone()));
        }
        menu = menu
            .child(menu_divider(pal))
            .child(self.menu_item("opt-rem", "Deadline Reminders", None, self.settings.reminders_enabled, true, pal, cx, |this, _w, cx| {
                this.settings.reminders_enabled = !this.settings.reminders_enabled;
                this.save_settings(cx);
            }))
            .child(self.menu_item("opt-fade", "Fade When Idle", None, self.settings.fade_when_idle, true, pal, cx, |this, _w, cx| {
                this.settings.fade_when_idle = !this.settings.fade_when_idle;
                this.save_settings(cx);
            }))
            .child(self.menu_item("opt-login", "Launch at Login", None, launch, true, pal, cx, |this, _w, cx| {
                crate::autostart::set(!crate::autostart::is_enabled());
                this.needs_sync = true;
                cx.notify();
            }))
            .child(menu_divider(pal))
            .child(self.menu_item("opt-clear", "Clear Completed", None, false, done > 0, pal, cx, |this, _w, cx| {
                this.overlay = None;
                this.edit_store(cx, |s| s.clear_completed());
            }))
            .child(menu_divider(pal))
            .child(self.menu_item("opt-quit", "Quit TodoBar", Some("Ctrl+Q".into()), false, true, pal, cx, |_this, _w, cx| cx.quit()));
        Self::abs_at(pos).child(menu).into_any_element()
    }

    fn google_lists_menu(&mut self, lists: Option<Vec<(String, String)>>, pos: Point<Pixels>, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let name = self.store.current_workspace().name.clone();
        let mut menu = menu_box(pal)
            .id("google-lists")
            .max_h(self.canvas.height - pos.y - px(8.))
            .overflow_y_scroll()
            .child(menu_label(&format!("Sync “{name}” with…"), pal));
        match lists {
            None => menu = menu.child(self.menu_item("gl-loading", "Loading…", None, false, false, pal, cx, |_t, _w, _c| {})),
            Some(lists) => {
                for (i, (id, title)) in lists.into_iter().enumerate() {
                    menu = menu.child(self.menu_item(format!("gl-{i}"), title, None, false, true, pal, cx, move |this, _w, cx| {
                        this.link_existing_list(id.clone(), cx)
                    }));
                }
                menu = menu.child(menu_divider(pal)).child(self.menu_item("gl-new", format!("New list “{name}”"), None, false, true, pal, cx, |this, _w, cx| {
                    this.link_new_list(cx)
                }));
            }
        }
        Self::abs_at(pos).child(menu).into_any_element()
    }

    fn task_menu(&mut self, id: uuid::Uuid, pos: Point<Pixels>, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.store.item(id).cloned() else { return div().into_any_element() };
        let many = self.store.workspaces.len() > 1;
        let mut menu = menu_box(pal)
            .child(self.menu_item("tm-done", if item.is_done { "Mark as Not Done" } else { "Mark as Done" }, None, false, true, pal, cx, move |this, _w, cx| {
                this.overlay = None;
                this.toggle_task(id, cx);
            }))
            .child(self.menu_item("tm-edit", if item.notes.is_none() { "Edit / Add Note" } else { "Edit" }, None, false, true, pal, cx, move |this, w, cx| {
                this.overlay = None;
                this.begin_editing(id, w, cx);
            }))
            .child(self.menu_item("tm-dl", if item.deadline.is_none() { "Add Deadline…" } else { "Change Deadline…" }, None, false, true, pal, cx, move |this, _w, cx| {
                this.open_task_deadline(id, cx);
            }));
        if item.deadline.is_some() {
            menu = menu.child(self.menu_item("tm-rmdl", "Remove Deadline", None, false, true, pal, cx, move |this, _w, cx| {
                this.overlay = None;
                this.edit_store(cx, |s| s.set_deadline(id, None, false, None));
            }));
        }
        if many {
            menu = menu.child(self.menu_item("tm-move", "Move to ▸", None, false, true, pal, cx, move |this, _w, cx| {
                this.overlay = Some(Overlay::MoveTo { id, pos });
                cx.notify();
            }));
        }
        menu = menu.child(menu_divider(pal)).child(self.menu_item("tm-del", "Delete", None, false, true, pal, cx, move |this, _w, cx| {
            this.overlay = None;
            this.delete_task(id, cx);
        }));
        Self::abs_at(pos).child(menu).into_any_element()
    }

    pub fn open_task_deadline(&mut self, id: uuid::Uuid, cx: &mut Context<Self>) {
        let Some(item) = self.store.item(id).cloned() else { return };
        let target = DeadlineTarget::Task(id);
        self.overlay = Some(Overlay::Deadline(match item.deadline {
            Some(d) => DeadlineState::from_existing(d, item.deadline_has_time, item.repeat_rule, true, target),
            None => DeadlineState::fresh(target),
        }));
        cx.notify();
    }

    fn move_to_menu(&mut self, id: uuid::Uuid, pos: Point<Pixels>, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let from = self.store.item(id).and_then(|i| i.workspace_id);
        let mut menu = menu_box(pal).child(menu_label("Move to", pal));
        for (i, ws) in self.store.workspaces.clone().into_iter().enumerate().filter(|(_, w)| Some(w.id) != from) {
            let target = ws.id;
            menu = menu.child(self.menu_item(format!("mv-{i}"), ws.name, None, false, true, pal, cx, move |this, _w, cx| {
                this.overlay = None;
                if this.selected == Some(id) {
                    this.selected = None;
                }
                this.edit_store(cx, |s| s.move_task_to_workspace(id, target));
            }));
        }
        Self::abs_at(pos).child(menu).into_any_element()
    }

    fn confirm_delete(&mut self, id: uuid::Uuid, pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.store.workspaces.iter().find(|w| w.id == id).cloned() else { return div().into_any_element() };
        let n = self.store.task_count(id);
        let plural = if n == 1 { "" } else { "s" };
        let r = self.rect;
        let w = 250.0f32.min(f32::from(r.size.width) - 20.0);
        let pos = point(r.origin.x + (r.size.width - px(w)) / 2.0, r.origin.y + px(60.));
        let btn = |label: &'static str, id: &'static str, danger: bool| {
            let mut b = div()
                .id(id)
                .px(px(12.))
                .py(px(4.))
                .rounded(px(6.))
                .cursor_pointer()
                .bg(if danger { pal.red } else { pal.hover })
                .text_color(if danger { gpui::white() } else { pal.fg })
                .child(label);
            b = b.hover(|s| s.opacity(0.85));
            b
        };
        Self::abs_at(pos)
            .w(px(w))
            .child(
                menu_box(pal)
                    .p(px(12.))
                    .gap(px(8.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Delete “{}”?", ws.name)))
                    .child(div().text_size(px(11.5)).text_color(pal.secondary).child(format!(
                        "Its {n} task{plural} will be deleted too. You can undo this with Ctrl+Z."
                    )))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(btn("Cancel", "cd-cancel", false).on_click(cx.listener(|this, _e, _w, cx| {
                                this.overlay = None;
                                cx.notify();
                            })))
                            .child(btn("Delete", "cd-delete", true).on_click(cx.listener(move |this, _e, _w, cx| {
                                this.delete_workspace(id, cx);
                            }))),
                    ),
            )
            .into_any_element()
    }
}
