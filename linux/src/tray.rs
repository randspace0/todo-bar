use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::*;
use ksni::MenuItem;
use uuid::Uuid;

use crate::cmd::Command;
use crate::model::Workspace;
use crate::settings::Theme;

#[derive(Debug)]
pub struct TodoTray {
    pub tx: Sender<Command>,
    pub workspaces: Vec<Workspace>,
    pub current: Uuid,
    pub theme: Theme,
    pub launch_at_login: bool,
    pub collapsed: bool,
}

fn item(label: &str, tx: &Sender<Command>, cmd: Command) -> MenuItem<TodoTray> {
    let tx = tx.clone();
    StandardItem {
        label: label.into(),
        activate: Box::new(move |_| {
            let _ = tx.send(cmd.clone());
        }),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for TodoTray {
    fn id(&self) -> String {
        "todobar".into()
    }

    fn title(&self) -> String {
        "TodoBar".into()
    }

    fn icon_name(&self) -> String {
        "todobar".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let tx = &self.tx;
        let mut menu = vec![
            item("Show TodoBar", tx, Command::Show),
            item(if self.collapsed { "Expand" } else { "Collapse to Side" }, tx, Command::ToggleCollapsed),
            MenuItem::Separator,
        ];
        for ws in &self.workspaces {
            let tx = tx.clone();
            let id = ws.id;
            menu.push(
                CheckmarkItem {
                    label: ws.name.clone(),
                    checked: ws.id == self.current,
                    activate: Box::new(move |_| {
                        let _ = tx.send(Command::SwitchWorkspace(id));
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.push(item("New Workspace…", tx, Command::NewWorkspace));
        menu.push(MenuItem::Separator);
        let txt = tx.clone();
        menu.push(
            SubMenu {
                label: "Theme".into(),
                submenu: vec![RadioGroup {
                    selected: Theme::ALL.iter().position(|t| *t == self.theme).unwrap_or(0),
                    select: Box::new(move |_, i| {
                        let _ = txt.send(Command::SetTheme(Theme::ALL[i]));
                    }),
                    options: Theme::ALL
                        .iter()
                        .map(|t| RadioItem { label: t.title().into(), ..Default::default() })
                        .collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
        );
        let txl = tx.clone();
        menu.push(
            CheckmarkItem {
                label: "Launch at Login".into(),
                checked: self.launch_at_login,
                activate: Box::new(move |_| {
                    let _ = txl.send(Command::ToggleLaunchAtLogin);
                }),
                ..Default::default()
            }
            .into(),
        );
        menu.push(MenuItem::Separator);
        menu.push(item("Quit TodoBar", tx, Command::Quit));
        menu
    }
}

/// Registers the tray icon; returns None (after logging one line) if no StatusNotifierWatcher exists.
pub fn spawn(tray: TodoTray) -> Option<Handle<TodoTray>> {
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("todobar: tray unavailable: {e}");
            None
        }
    }
}
