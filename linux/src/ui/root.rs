use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use gpui::{
    actions, div, point, prelude::*, px, size, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding,
    Pixels, Point, Size, Window, WindowAppearance,
};
use ksni::blocking::Handle;
use uuid::Uuid;

use crate::anim::{Anim, Tween};
use crate::autostart;
use crate::cmd::Command;
use crate::model::{RepeatRule, TodoItem};
use crate::reminders::{Reminders, Snapshot};
use crate::settings::{Edge, SavedRect, Settings, SortMode, Theme, WindowStyle};
use crate::store::Store;
use crate::tray::TodoTray;
use crate::ui::deadline_editor::DeadlineState;
use crate::ui::glass::Palette;
use crate::ui::text_input::{InputEvent, TextInput};

actions!(
    todobar,
    [
        CollapseKey, SelUp, SelDown, ToggleSel, EditSel, DeleteSel, MoveUp, MoveDown, NewTask, UndoKey,
        RedoKey, QuitKey, NewWorkspaceKey, Ws1, Ws2, Ws3, Ws4, Ws5, Ws6, Ws7, Ws8, Ws9,
    ]
);

pub fn bind_keys(cx: &mut gpui::App) {
    let nav = Some("Panel && !TodoInput");
    let panel = Some("Panel");
    cx.bind_keys([
        KeyBinding::new("escape", CollapseKey, nav),
        KeyBinding::new("up", SelUp, nav),
        KeyBinding::new("down", SelDown, nav),
        KeyBinding::new("space", ToggleSel, nav),
        KeyBinding::new("enter", EditSel, nav),
        KeyBinding::new("backspace", DeleteSel, nav),
        KeyBinding::new("delete", DeleteSel, nav),
        KeyBinding::new("ctrl-up", MoveUp, nav),
        KeyBinding::new("ctrl-down", MoveDown, nav),
        KeyBinding::new("ctrl-n", NewTask, panel),
        KeyBinding::new("ctrl-z", UndoKey, panel),
        KeyBinding::new("ctrl-shift-z", RedoKey, panel),
        KeyBinding::new("ctrl-q", QuitKey, panel),
        KeyBinding::new("ctrl-shift-n", NewWorkspaceKey, panel),
        KeyBinding::new("ctrl-1", Ws1, panel),
        KeyBinding::new("ctrl-2", Ws2, panel),
        KeyBinding::new("ctrl-3", Ws3, panel),
        KeyBinding::new("ctrl-4", Ws4, panel),
        KeyBinding::new("ctrl-5", Ws5, panel),
        KeyBinding::new("ctrl-6", Ws6, panel),
        KeyBinding::new("ctrl-7", Ws7, panel),
        KeyBinding::new("ctrl-8", Ws8, panel),
        KeyBinding::new("ctrl-9", Ws9, panel),
    ]);
}

pub const TAB_SIZE: Size<Pixels> = Size { width: px(30.0), height: px(96.0) };
pub const MIN_SIZE: Size<Pixels> = Size { width: px(250.0), height: px(200.0) };
pub const IDLE_ALPHA: f32 = 0.4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CanvasMode {
    /// Full-output layer-shell surface; the panel is drawn at `rect` inside it.
    Canvas,
    /// Normal toplevel (compositor lacks layer-shell); the panel fills the window.
    Toplevel,
}

pub enum Overlay {
    Workspaces,
    Options,
    Deadline(DeadlineState),
    TaskMenu { id: Uuid, pos: Point<Pixels> },
    MoveTo { id: Uuid, pos: Point<Pixels> },
    ConfirmDeleteWorkspace(Uuid),
}

#[derive(Clone, Copy)]
pub enum Drag {
    Move { start_mouse: Point<Pixels>, start_origin: Point<Pixels> },
    Resize { start_mouse: Point<Pixels>, start_size: Size<Pixels> },
    Tab { start_mouse: Point<Pixels>, start_origin: Point<Pixels>, moved: bool },
    Reorder { id: Uuid, start_mouse: Point<Pixels>, active: bool },
}

#[derive(Clone, Copy)]
pub struct PendingDeadline {
    pub date: DateTime<Utc>,
    pub has_time: bool,
    pub repeat: Option<RepeatRule>,
}

pub struct Root {
    pub store: Store,
    pub settings: Settings,
    pub focus: FocusHandle,
    pub add_input: Entity<TextInput>,
    pub title_input: Entity<TextInput>,
    pub notes_input: Entity<TextInput>,
    pub ws_input: Entity<TextInput>,
    reminders: Reminders,
    tray: Option<Handle<TodoTray>>,
    commands_rx: Receiver<Command>,
    pub needs_sync: bool,
    last_tray_sig: String,

    pub mode: CanvasMode,
    pub canvas: Size<Pixels>,
    pub rect: Bounds<Pixels>,
    pub expanded: Bounds<Pixels>,
    pub collapsed: bool,
    pub edge: Edge,
    pub anim: Option<Anim>,
    pub drag: Option<Drag>,
    last_region: Option<Option<Bounds<Pixels>>>,

    pub mouse_inside: bool,
    pub last_activity: Instant,
    pub alpha: Tween,

    pub selected: Option<Uuid>,
    pub editing: Option<Uuid>,
    pub renaming_ws: bool,
    pub expanded_notes: HashSet<Uuid>,
    pub settling: HashMap<Uuid, Instant>,
    pub check_anim: HashMap<Uuid, (bool, Instant)>,
    /// Rows already shown at least once; a row not in here fades in on first render.
    pub known_rows: HashSet<Uuid>,
    pub appear: HashMap<Uuid, Instant>,
    pub list_fade: Option<Instant>,
    pub dbg_row: usize,
    pub overlay: Option<Overlay>,
    pub new_deadline: Option<PendingDeadline>,
    pub reorder_active: Option<Uuid>,
    last_minute: i64,
}

impl Root {
    pub fn new(
        canvas: Size<Pixels>,
        mode: CanvasMode,
        commands_tx: Sender<Command>,
        commands_rx: Receiver<Command>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = Settings::load();
        if !settings.login_item_configured {
            settings.login_item_configured = true;
            autostart::set(true);
            settings.save();
        }
        let store = Store::load();
        let saved = settings.expanded.unwrap_or(SavedRect {
            x: f32::from(canvas.width) - 290.0 - 20.0,
            y: 20.0,
            w: 290.0,
            h: 380.0,
        });
        let expanded = Bounds {
            origin: point(px(saved.x), px(saved.y)),
            size: size(px(saved.w.max(250.0)), px(saved.h.max(200.0))),
        };

        let add_input = cx.new(|cx| TextInput::new("Add a task…", false, cx));
        let title_input = cx.new(|cx| TextInput::new("Title", false, cx));
        let notes_input = cx.new(|cx| TextInput::new("Notes or link (Alt+Enter for new line)", true, cx));
        let ws_input = cx.new(|cx| TextInput::new("Workspace name", false, cx));
        let focus = cx.focus_handle();

        cx.subscribe_in(&add_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Submit => this.add_task(cx),
            InputEvent::Cancel => this.collapse(window, cx),
            InputEvent::Down => {
                this.move_selection(1, window, cx);
            }
            InputEvent::Up => {}
        })
        .detach();
        for input in [&title_input, &notes_input] {
            let is_title = input == &title_input;
            cx.subscribe_in(input, window, move |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.commit_edit(window, cx),
                InputEvent::Cancel => this.cancel_edit(window, cx),
                InputEvent::Down if is_title => {
                    let h = this.notes_input.focus_handle(cx);
                    window.focus(&h, cx);
                }
                InputEvent::Up if !is_title => {
                    let h = this.title_input.focus_handle(cx);
                    window.focus(&h, cx);
                }
                _ => {}
            })
            .detach();
        }
        cx.subscribe_in(&ws_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Submit => this.commit_ws_name(window, cx),
            InputEvent::Cancel => {
                this.renaming_ws = false;
                window.focus(&this.focus.clone(), cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();

        // Focus-out commits (blur commits in the Swift app).
        let (th, nh) = (title_input.focus_handle(cx), notes_input.focus_handle(cx));
        for h in [th.clone(), nh.clone()] {
            let (th, nh) = (th.clone(), nh.clone());
            cx.on_focus_out(&h, window, move |this, _ev, window, cx| {
                if this.editing.is_some() && !th.contains_focused(window, cx) && !nh.contains_focused(window, cx) {
                    this.commit_edit(window, cx);
                }
            })
            .detach();
        }
        let wh = ws_input.focus_handle(cx);
        cx.on_focus_out(&wh, window, |this, _ev, window, cx| {
            if this.renaming_ws {
                this.commit_ws_name(window, cx);
            }
        })
        .detach();
        let ah = add_input.focus_handle(cx);
        cx.on_focus_in(&ah, window, |this, _window, cx| {
            if this.selected.take().is_some() {
                cx.notify();
            }
        })
        .detach();

        let reminders = Reminders::spawn(commands_tx.clone());
        let tray = crate::tray::spawn(TodoTray {
            tx: commands_tx.clone(),
            workspaces: store.workspaces.clone(),
            current: store.current_workspace_id,
            theme: settings.theme,
            launch_at_login: autostart::is_enabled(),
            collapsed: settings.is_collapsed,
        });

        let collapsed = settings.is_collapsed;
        let edge = settings.edge;
        let rect = if mode == CanvasMode::Canvas && collapsed {
            tab_rect(canvas, edge, px(settings.tab_y))
        } else if mode == CanvasMode::Canvas {
            clamp_rect(expanded, canvas)
        } else {
            Bounds { origin: point(px(0.), px(0.)), size: canvas }
        };

        let mut root = Self {
            store,
            settings,
            focus,
            add_input,
            title_input,
            notes_input,
            ws_input,
            reminders,
            tray,
            commands_rx,
            needs_sync: true,
            last_tray_sig: String::new(),
            mode,
            canvas,
            rect,
            expanded,
            collapsed,
            edge,
            anim: None,
            drag: None,
            last_region: None,
            mouse_inside: false,
            last_activity: Instant::now(),
            alpha: Tween::new(1.0),
            selected: None,
            editing: None,
            renaming_ws: false,
            expanded_notes: HashSet::new(),
            settling: HashMap::new(),
            check_anim: HashMap::new(),
            known_rows: HashSet::new(),
            appear: HashMap::new(),
            list_fade: None,
            dbg_row: 0,
            overlay: None,
            new_deadline: None,
            reorder_active: None,
            last_minute: 0,
        };
        root.mark_rows_known();
        root.sync_background(cx);
        root.start_tick(window, cx);
        window.focus(&root.focus.clone(), cx);
        root
    }

    // MARK: background loop

    fn start_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let ok = this.update_in(cx, |this, window, cx| this.tick(window, cx));
            if ok.is_err() {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(cmd) = self.commands_rx.try_recv() {
            self.handle_command(cmd, window, cx);
        }
        let now = Instant::now();
        let before = self.settling.len();
        self.settling.retain(|_, t| now.duration_since(*t) < Duration::from_millis(800));
        self.check_anim.retain(|_, (_, t)| now.duration_since(*t) < Duration::from_millis(400));
        self.appear.retain(|_, t| now.duration_since(*t) < Duration::from_millis(250));
        if self.list_fade.is_some_and(|t| now.duration_since(t) > Duration::from_millis(200)) {
            self.list_fade = None;
        }
        if self.settling.len() != before {
            cx.notify();
        }
        let minute = Utc::now().timestamp() / 60;
        if minute != self.last_minute {
            self.last_minute = minute;
            cx.notify();
        }
        let want = self.desired_alpha(window, cx);
        if (self.alpha.target() - want).abs() > f32::EPSILON {
            cx.notify();
        }
        if self.needs_sync {
            self.sync_background(cx);
        }
    }

    fn sync_background(&mut self, _cx: &mut Context<Self>) {
        self.needs_sync = false;
        self.reminders.update(Snapshot {
            items: self.store.all_items.clone(),
            workspaces: self.store.workspaces.clone(),
            enabled: self.settings.reminders_enabled,
        });
        let launch = autostart::is_enabled();
        let sig = format!(
            "{:?}{:?}{:?}{}{}",
            self.store.workspaces, self.store.current_workspace_id, self.settings.theme, launch, self.collapsed
        );
        if sig != self.last_tray_sig {
            self.last_tray_sig = sig;
            if let Some(h) = &self.tray {
                let ws = self.store.workspaces.clone();
                let cur = self.store.current_workspace_id;
                let theme = self.settings.theme;
                let collapsed = self.collapsed;
                h.update(move |t| {
                    t.workspaces = ws;
                    t.current = cur;
                    t.theme = theme;
                    t.launch_at_login = launch;
                    t.collapsed = collapsed;
                });
            }
        }
    }

    /// Run a store mutation and refresh everything that depends on it.
    pub fn edit_store(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Store)) {
        f(&mut self.store);
        self.needs_sync = true;
        self.touch();
        cx.notify();
    }

    pub fn save_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        self.needs_sync = true;
        cx.notify();
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    fn handle_command(&mut self, cmd: Command, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Command::Show => self.show(window, cx),
            Command::ToggleCollapsed => {
                if self.collapsed {
                    self.expand(window, cx)
                } else {
                    self.collapse(window, cx)
                }
            }
            Command::SwitchWorkspace(id) => self.switch_workspace(id, window, cx),
            Command::NewWorkspace => {
                self.show(window, cx);
                self.new_workspace(window, cx);
            }
            Command::SetTheme(t) => {
                self.settings.theme = t;
                self.save_settings(cx);
            }
            Command::ToggleLaunchAtLogin => {
                autostart::set(!autostart::is_enabled());
                self.needs_sync = true;
                cx.notify();
            }
            Command::OpenTask(id) => {
                self.show(window, cx);
                if let Some(ws) = self.store.item(id).and_then(|i| i.workspace_id) {
                    self.switch_workspace(ws, window, cx);
                }
                self.selected = Some(id);
                cx.notify();
            }
            Command::Quit => cx.quit(),
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expand(window, cx);
        window.activate_window();
    }

    // MARK: theme

    pub fn is_dark(&self, window: &Window) -> bool {
        match self.settings.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => !matches!(window.appearance(), WindowAppearance::Light | WindowAppearance::VibrantLight),
        }
    }

    pub fn palette(&self, window: &Window) -> Palette {
        Palette::new(self.is_dark(window))
    }

    pub fn toggle_light_dark(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.settings.theme = if self.is_dark(window) { Theme::Light } else { Theme::Dark };
        self.save_settings(cx);
    }

    // MARK: fade

    fn text_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        [&self.add_input, &self.title_input, &self.notes_input, &self.ws_input]
            .iter()
            .any(|i| i.read(cx).is_focused(window))
    }

    fn desired_alpha(&self, window: &Window, cx: &gpui::App) -> f32 {
        let opaque = !self.settings.fade_when_idle
            || self.collapsed
            || self.mouse_inside
            || self.drag.is_some()
            || self.overlay.is_some()
            || window.is_window_active()
            || self.text_focused(window, cx)
            || self.last_activity.elapsed() < Duration::from_millis(1500);
        if opaque {
            1.0
        } else {
            IDLE_ALPHA
        }
    }

    // MARK: geometry

    /// Advance animations, input region and fade; called from render.
    pub fn frame_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vp = window.viewport_size();
        if self.mode == CanvasMode::Canvas && vp != self.canvas && vp.width > px(0.) && vp.height > px(0.) {
            self.canvas = vp;
            self.rect = if self.collapsed {
                tab_rect(vp, self.edge, self.rect.origin.y)
            } else {
                clamp_rect(self.rect, vp)
            };
            self.expanded = clamp_rect(self.expanded, vp);
        }
        if self.mode == CanvasMode::Toplevel {
            self.canvas = vp;
            self.rect = Bounds { origin: point(px(0.), px(0.)), size: vp };
        }
        let mut animating = false;
        if let Some(anim) = self.anim {
            self.rect = anim.current();
            if anim.done() {
                self.anim = None;
                self.persist(cx);
            } else {
                animating = true;
            }
        }
        let want = self.desired_alpha(window, cx);
        self.alpha.set_target(want, Duration::from_millis(300));
        if self.alpha.active() {
            animating = true;
        }
        if !self.check_anim.is_empty() || !self.settling.is_empty() || !self.appear.is_empty() || self.list_fade.is_some() {
            animating = true;
        }
        if animating {
            window.request_animation_frame();
        }

        if self.mode == CanvasMode::Canvas {
            let region = if self.drag.is_some() || self.overlay.is_some() {
                None
            } else {
                Some(self.rect)
            };
            if self.last_region != Some(region) {
                match region {
                    Some(r) => window.set_input_region(Some(&[r])),
                    None => window.set_input_region(None),
                }
                self.last_region = Some(region);
            }
        }
    }

    pub fn persist(&mut self, _cx: &mut Context<Self>) {
        if self.mode == CanvasMode::Canvas {
            if !self.collapsed {
                self.expanded = self.rect;
            } else {
                self.settings.tab_y = f32::from(self.rect.origin.y);
            }
            let e = self.expanded;
            self.settings.expanded = Some(SavedRect {
                x: f32::from(e.origin.x),
                y: f32::from(e.origin.y),
                w: f32::from(e.size.width),
                h: f32::from(e.size.height),
            });
        }
        self.settings.is_collapsed = self.collapsed;
        self.settings.edge = self.edge;
        self.settings.save();
    }

    pub fn start_anim(&mut self, to: Bounds<Pixels>) {
        self.anim = Some(Anim::new(self.rect, to, Duration::from_millis(220)));
    }

    pub fn collapse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.collapsed {
            return;
        }
        self.overlay = None;
        self.clear_selection();
        self.editing = None;
        self.renaming_ws = false;
        window.focus(&self.focus.clone(), cx);
        self.collapsed = true;
        self.touch();
        if self.mode == CanvasMode::Canvas {
            self.expanded = self.rect;
            self.edge = if self.rect.center().x < px(f32::from(self.canvas.width) / 2.0) { Edge::Left } else { Edge::Right };
            let to = tab_rect(self.canvas, self.edge, self.rect.origin.y);
            self.start_anim(to);
        } else {
            self.expanded = self.rect;
            window.resize(TAB_SIZE);
        }
        self.persist(cx);
        self.needs_sync = true;
        cx.notify();
    }

    pub fn expand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.collapsed {
            return;
        }
        self.collapsed = false;
        self.touch();
        if self.mode == CanvasMode::Canvas {
            let mut target = self.expanded;
            let w = target.size.width;
            target.origin.y = self.rect.origin.y;
            target.origin.x = match self.edge {
                Edge::Left => px(12.),
                Edge::Right => self.canvas.width - w - px(12.),
            };
            let target = clamp_rect(target, self.canvas);
            self.expanded = target;
            self.start_anim(target);
        } else {
            window.resize(self.expanded.size);
        }
        window.activate_window();
        window.focus(&self.focus.clone(), cx);
        self.persist(cx);
        self.needs_sync = true;
        cx.notify();
    }

    pub fn snap_tab(&mut self, cx: &mut Context<Self>) {
        self.edge = if self.rect.center().x < px(f32::from(self.canvas.width) / 2.0) { Edge::Left } else { Edge::Right };
        let to = tab_rect(self.canvas, self.edge, self.rect.origin.y);
        self.start_anim(to);
        cx.notify();
    }

    // MARK: selection / lists

    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    fn is_open_section(&self, item: &TodoItem) -> bool {
        !item.is_done || self.settling.contains_key(&item.id)
    }

    pub fn open_items(&self) -> Vec<TodoItem> {
        let open: Vec<TodoItem> = self.store.items().into_iter().filter(|i| self.is_open_section(i)).collect();
        if self.settings.sort_mode != SortMode::Deadline {
            return open;
        }
        let mut keyed: Vec<(usize, TodoItem)> = open.into_iter().enumerate().collect();
        keyed.sort_by(|(ia, a), (ib, b)| {
            let da = a.deadline.unwrap_or(DateTime::<Utc>::MAX_UTC);
            let db = b.deadline.unwrap_or(DateTime::<Utc>::MAX_UTC);
            da.cmp(&db).then(ia.cmp(ib))
        });
        keyed.into_iter().map(|(_, i)| i).collect()
    }

    pub fn done_items(&self) -> Vec<TodoItem> {
        let mut done: Vec<TodoItem> = self.store.items().into_iter().filter(|i| !self.is_open_section(i)).collect();
        done.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
        done
    }

    fn navigable(&self) -> Vec<TodoItem> {
        let mut v = self.open_items();
        if self.settings.show_done {
            v.extend(self.done_items());
        }
        v
    }

    pub fn can_reorder(&self, item: &TodoItem) -> bool {
        self.settings.sort_mode == SortMode::Manual && self.is_open_section(item)
    }

    pub fn move_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.navigable();
        if list.is_empty() {
            return;
        }
        window.focus(&self.focus.clone(), cx);
        let cur = self.selected.and_then(|id| list.iter().position(|i| i.id == id));
        match cur {
            None => {
                self.selected = Some(if delta > 0 { list[0].id } else { list[list.len() - 1].id });
            }
            Some(i) => {
                let t = i as isize + delta;
                if t < 0 {
                    self.focus_add(window, cx);
                } else if (t as usize) < list.len() {
                    self.selected = Some(list[t as usize].id);
                }
            }
        }
        cx.notify();
    }

    pub fn focus_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        let h = self.add_input.focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    pub fn move_selected_task(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.settings.sort_mode != SortMode::Manual {
            return;
        }
        let Some(id) = self.selected else { return };
        let open: Vec<TodoItem> = self.open_items().into_iter().filter(|i| !i.is_done).collect();
        let Some(i) = open.iter().position(|t| t.id == id) else { return };
        let t = i as isize + delta;
        if t < 0 || t as usize >= open.len() {
            return;
        }
        let target = open[t as usize].id;
        self.edit_store(cx, |s| s.move_(id, target));
    }

    // MARK: task actions

    pub fn toggle_task(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(item) = self.store.item(id) else { return };
        let was_done = item.is_done;
        self.check_anim.insert(id, (!was_done, Instant::now()));
        if !was_done {
            self.settling.insert(id, Instant::now());
        }
        self.edit_store(cx, |s| s.toggle(id));
    }

    pub fn delete_task(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let list = self.navigable();
        if self.selected == Some(id) {
            if let Some(i) = list.iter().position(|t| t.id == id) {
                self.selected = if i + 1 < list.len() {
                    Some(list[i + 1].id)
                } else if i > 0 {
                    Some(list[i - 1].id)
                } else {
                    None
                };
            }
        }
        if self.editing == Some(id) {
            self.editing = None;
        }
        self.edit_store(cx, |s| s.delete(id));
    }

    pub fn begin_editing(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.store.item(id).cloned() else { return };
        self.selected = Some(id);
        self.editing = Some(id);
        self.title_input.update(cx, |i, cx| i.set_text(&item.title, window, cx));
        self.notes_input.update(cx, |i, cx| i.set_text(item.notes.as_deref().unwrap_or(""), window, cx));
        let h = self.title_input.focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing.take() else { return };
        let title = self.title_input.read(cx).text();
        let notes = self.notes_input.read(cx).text();
        self.edit_store(cx, |s| s.edit(id, &title, &notes));
        window.focus(&self.focus.clone(), cx);
    }

    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        window.focus(&self.focus.clone(), cx);
        cx.notify();
    }

    pub fn add_task(&mut self, cx: &mut Context<Self>) {
        let title = self.add_input.read(cx).text();
        if title.trim().is_empty() {
            return;
        }
        let pending = self.new_deadline.take();
        self.edit_store(cx, |s| {
            s.add(&title, pending.map(|p| p.date), pending.is_some_and(|p| p.has_time), pending.and_then(|p| p.repeat))
        });
        // Clear the field; needs a window for notify, so defer.
        let input = self.add_input.clone();
        cx.defer(move |cx| {
            input.update(cx, |i, cx| {
                i.content = "".into();
                cx.notify();
            })
        });
    }

    // MARK: workspaces

    /// Every current task counts as already shown, so only tasks added later fade in.
    pub fn mark_rows_known(&mut self) {
        self.known_rows = self.store.all_items.iter().map(|i| i.id).collect();
    }

    pub fn switch_workspace(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if id == self.store.current_workspace_id {
            return;
        }
        self.clear_selection();
        self.editing = None;
        self.renaming_ws = false;
        self.overlay = None;
        window.focus(&self.focus.clone(), cx);
        self.edit_store(cx, |s| s.switch_workspace(id));
        self.mark_rows_known();
        self.list_fade = Some(Instant::now());
    }

    pub fn switch_workspace_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ws) = self.store.workspaces.get(index) {
            let id = ws.id;
            self.switch_workspace(id, window, cx);
        }
    }

    pub fn new_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        self.overlay = None;
        self.edit_store(cx, |s| {
            s.add_workspace("New Workspace");
        });
        self.begin_rename(window, cx);
    }

    pub fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = None;
        self.renaming_ws = true;
        let name = self.store.current_workspace().name.clone();
        self.ws_input.update(cx, |i, cx| i.set_text(&name, window, cx));
        let h = self.ws_input.focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    pub fn commit_ws_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.renaming_ws {
            return;
        }
        self.renaming_ws = false;
        let name = self.ws_input.read(cx).text();
        let id = self.store.current_workspace_id;
        self.edit_store(cx, |s| s.rename_workspace(id, &name));
        window.focus(&self.focus.clone(), cx);
    }

    pub fn request_delete_workspace(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.store.workspaces.len() < 2 {
            return;
        }
        if self.store.task_count(id) > 0 {
            self.overlay = Some(Overlay::ConfirmDeleteWorkspace(id));
            cx.notify();
        } else {
            self.delete_workspace(id, cx);
        }
    }

    pub fn delete_workspace(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.clear_selection();
        self.renaming_ws = false;
        self.overlay = None;
        self.edit_store(cx, |s| s.delete_workspace(id));
    }

    // MARK: undo

    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        self.editing = None;
        window.focus(&self.focus.clone(), cx);
        self.edit_store(cx, |s| s.undo());
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection();
        self.editing = None;
        window.focus(&self.focus.clone(), cx);
        self.edit_store(cx, |s| s.redo());
    }

    pub fn set_window_style(&mut self, style: WindowStyle, cx: &mut Context<Self>) {
        self.settings.window_style = style;
        self.save_settings(cx);
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

pub fn clamp_rect(mut r: Bounds<Pixels>, canvas: Size<Pixels>) -> Bounds<Pixels> {
    r.size.width = r.size.width.min(canvas.width);
    r.size.height = r.size.height.min(canvas.height);
    r.origin.x = r.origin.x.max(px(0.)).min(canvas.width - r.size.width);
    r.origin.y = r.origin.y.max(px(0.)).min(canvas.height - r.size.height);
    r
}

pub fn tab_rect(canvas: Size<Pixels>, edge: Edge, top: Pixels) -> Bounds<Pixels> {
    let y = top.max(px(0.)).min(canvas.height - TAB_SIZE.height);
    let x = match edge {
        Edge::Left => px(0.),
        Edge::Right => canvas.width - TAB_SIZE.width,
    };
    Bounds { origin: point(x, y), size: TAB_SIZE }
}

#[allow(unused)]
fn _unused(_: &gpui::Div) {
    let _ = div();
}

#[cfg(test)]
mod smoke {
    use super::*;
    use gpui::{MouseButton, TestAppContext, VisualTestContext};
    use std::sync::mpsc;

    #[gpui::test]
    fn keyboard_flow(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("todobar-smoke-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        cx.update(|cx| {
            crate::ui::text_input::bind_keys(cx);
            bind_keys(cx);
        });
        let (tx, rx) = mpsc::channel();
        let window = cx.add_window(|window, cx| {
            Root::new(size(px(1920.), px(1080.)), CanvasMode::Canvas, tx, rx, window, cx)
        });
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        window.update(cx, |root, window, cx| root.focus_add(window, cx)).unwrap();

        cx.simulate_input("buy milk");
        cx.simulate_keystrokes("enter");
        let items = window.read_with(cx, |r, _| r.store.items()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "buy milk");

        cx.simulate_keystrokes("down space");
        assert!(window.read_with(cx, |r, _| r.store.items()[0].is_done).unwrap());
        cx.simulate_keystrokes("ctrl-z");
        assert!(!window.read_with(cx, |r, _| r.store.items()[0].is_done).unwrap());

        cx.simulate_keystrokes("ctrl-shift-n");
        assert_eq!(window.read_with(cx, |r, _| r.store.workspaces.len()).unwrap(), 2);
        cx.simulate_input("Home");
        cx.simulate_keystrokes("enter");
        assert_eq!(window.read_with(cx, |r, _| r.store.current_workspace().name.clone()).unwrap(), "New WorkspaceHome");
        cx.simulate_keystrokes("ctrl-1");
        assert_eq!(window.read_with(cx, |r, _| r.store.items().len()).unwrap(), 1);

        cx.simulate_keystrokes("escape");
        assert!(window.read_with(cx, |r, _| r.collapsed).unwrap());

        // Mouse: drag the header, then click the tab to expand again.
        cx.simulate_keystrokes("ctrl-n");
        let wait = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(std::time::Duration::from_millis(400));
            std::thread::sleep(std::time::Duration::from_millis(400));
            cx.run_until_parked();
        };
        wait(cx);
        assert!(!window.read_with(cx, |r, _| r.collapsed).unwrap());
        // Finish the expand animation deterministically (the test clock is not the animation clock).
        window.update(cx, |r, _, _| { r.anim = None; r.rect = r.expanded; }).unwrap();
        let before = window.read_with(cx, |r, _| r.rect).unwrap();
        cx.simulate_mouse_move(point(before.origin.x + px(150.), before.origin.y + px(18.)), None, gpui::Modifiers::none());
        cx.simulate_mouse_down(point(before.origin.x + px(150.), before.origin.y + px(18.)), MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_move(point(before.origin.x + px(50.), before.origin.y + px(218.)), MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(point(before.origin.x + px(50.), before.origin.y + px(218.)), MouseButton::Left, gpui::Modifiers::none());
        let after = window.read_with(cx, |r, _| r.rect).unwrap();
        assert_eq!(after.origin.x, before.origin.x - px(100.));
        assert_eq!(after.origin.y, before.origin.y + px(200.));

        cx.simulate_keystrokes("escape");
        let tab = window.update(cx, |r, _, _| {
            let to = r.anim.expect("collapse animates").to;
            r.anim = None;
            r.rect = to;
            to
        }).unwrap();
        cx.run_until_parked();
        assert_eq!(tab.size, TAB_SIZE);
        assert_eq!(tab.origin.x, px(1920. - 30.));
        let c = tab.center();
        cx.simulate_mouse_move(c, None, gpui::Modifiers::none());
        cx.simulate_mouse_down(c, MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(c, MouseButton::Left, gpui::Modifiers::none());
        assert!(!window.read_with(cx, |r, _| r.collapsed).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn setup(cx: &mut TestAppContext, tag: &str) -> (gpui::WindowHandle<Root>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("todobar-{tag}-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        cx.update(|cx| {
            crate::ui::text_input::bind_keys(cx);
            bind_keys(cx);
        });
        let (tx, rx) = mpsc::channel();
        let window = cx.add_window(|window, cx| {
            Root::new(size(px(1920.), px(1080.)), CanvasMode::Canvas, tx, rx, window, cx)
        });
        (window, dir)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let b = cx.debug_bounds(selector).unwrap_or_else(|| panic!("no element {selector}"));
        let c = b.center();
        cx.simulate_mouse_move(c, None, gpui::Modifiers::none());
        cx.simulate_click(c, gpui::Modifiers::none());
    }

    #[gpui::test]
    fn reorder_by_mouse_drag(cx: &mut TestAppContext) {
        let (window, dir) = setup(cx, "reorder");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        window.update(cx, |r, _, cx| {
            for t in ["a", "b", "c"] {
                r.edit_store(cx, |s| s.add(t, None, false, None));
            }
        }).unwrap();
        cx.run_until_parked();
        cx.draw(point(px(0.), px(0.)), size(px(1920.), px(1080.)), |_, _| div());
        let titles = |cx: &mut VisualTestContext| window.read_with(cx, |r, _| r.store.items().iter().map(|i| i.title.clone()).collect::<Vec<_>>()).unwrap();
        assert_eq!(titles(cx), ["a", "b", "c"]);
        let first = cx.debug_bounds("row-0").expect("row-0").center();
        let last = cx.debug_bounds("row-2").expect("row-2").center();
        cx.simulate_mouse_move(first, None, gpui::Modifiers::none());
        cx.simulate_mouse_down(first, MouseButton::Left, gpui::Modifiers::none());
        // First move past the drag threshold, then onto the last row.
        cx.simulate_mouse_move(point(first.x, first.y + px(10.)), MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_move(last, MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(last, MouseButton::Left, gpui::Modifiers::none());
        assert_eq!(titles(cx), ["b", "c", "a"]);
        // One drag = one undo step.
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(titles(cx), ["a", "b", "c"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui::test]
    fn menus_by_click(cx: &mut TestAppContext) {
        let (window, dir) = setup(cx, "menus");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        let fade = |cx: &mut VisualTestContext| window.read_with(cx, |r, _| r.settings.fade_when_idle).unwrap();
        assert!(fade(cx));
        click(cx, "options-btn");
        assert!(window.read_with(cx, |r, _| matches!(r.overlay, Some(Overlay::Options))).unwrap());
        click(cx, "opt-fade");
        assert!(!fade(cx));
        // Workspace menu: New Workspace creates and switches to a second workspace.
        window.update(cx, |r, _, _| r.overlay = None).unwrap();
        click(cx, "ws-btn");
        click(cx, "ws-new");
        assert_eq!(window.read_with(cx, |r, _| r.store.workspaces.len()).unwrap(), 2);
        assert!(window.read_with(cx, |r, _| r.renaming_ws).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui::test]
    fn deadline_popover(cx: &mut TestAppContext) {
        let (window, dir) = setup(cx, "deadline");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        click(cx, "new-deadline");
        assert!(window.read_with(cx, |r, _| matches!(r.overlay, Some(Overlay::Deadline(_)))).unwrap());
        click(cx, "Tomorrow");
        let pending = window.read_with(cx, |r, _| r.new_deadline).unwrap().expect("deadline chosen");
        let tomorrow = chrono::Local::now().date_naive() + chrono::Duration::days(1);
        assert_eq!(pending.date.with_timezone(&chrono::Local).date_naive(), tomorrow);
        assert!(!pending.has_time);
        window.update(cx, |r, window, cx| {
            r.add_input.update(cx, |i, cx| i.set_text("pay bills", window, cx));
            r.add_task(cx);
        }).unwrap();
        let item = window.read_with(cx, |r, _| r.store.items()[0].clone()).unwrap();
        assert_eq!(item.title, "pay bills");
        cx.run_until_parked();
        assert!(window.read_with(cx, |r, _| r.appear.contains_key(&item.id)).unwrap(), "new row fades in");
        assert_eq!(item.deadline.unwrap().with_timezone(&chrono::Local).date_naive(), tomorrow);
        // Row editor: change the deadline on the task via the popover and "Set".
        window.update(cx, |r, _, cx| r.open_task_deadline(item.id, cx)).unwrap();
        click(cx, "dl-set");
        assert!(window.read_with(cx, |r, _| r.overlay.is_none()).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}
