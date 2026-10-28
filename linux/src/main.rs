mod anim;
mod autostart;
mod cmd;
mod model;
mod reminders;
mod settings;
mod store;
mod tray;
mod ui;

use std::borrow::Cow;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};

use gpui::{
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point, px, size, App, AppContext, AssetSource, Bounds, SharedString, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowKind, WindowOptions,
};
use gpui_platform::application;

use crate::cmd::Command;
use crate::ui::root::{CanvasMode, Root};

struct Assets;

macro_rules! icons {
    ($($name:literal),*) => {
        fn icon_bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $name, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice()),)*
                _ => None,
            }
        }
        const ICON_NAMES: &[&str] = &[$(concat!("icons/", $name, ".svg")),*];
    };
}

icons!(
    "plus", "check", "x", "ellipsis", "sun", "moon", "collapse", "calendar", "calendar-plus", "repeat",
    "chevron-down", "chevron-up", "chevron-right", "chevron-left", "alert-circle"
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICON_NAMES.iter().map(|s| SharedString::from(*s)).collect())
    }
}

fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("TODOBAR_SOCKET") {
        return PathBuf::from(p);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("todobar.sock")
}

/// Single instance: if one is already running, ask it to show itself and exit.
fn claim_instance(tx: Sender<Command>) {
    let path = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"show\n");
        std::process::exit(0);
    }
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => {
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut buf = [0u8; 16];
                    let _ = (&stream).read(&mut buf);
                    let _ = tx.send(Command::Show);
                }
            });
        }
        Err(e) => eprintln!("todobar: single-instance socket unavailable: {e}"),
    }
}

fn open_root(
    cx: &mut App,
    mode: CanvasMode,
    canvas: gpui::Size<gpui::Pixels>,
    tx: &Sender<Command>,
    rx: &Rc<RefCell<Option<Receiver<Command>>>>,
) -> anyhow::Result<()> {
    let kind = match mode {
        CanvasMode::Canvas => WindowKind::LayerShell(LayerShellOptions {
            namespace: "todobar".to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: None,
            exclusive_edge: None,
            margin: None,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
        }),
        CanvasMode::Toplevel => WindowKind::Normal,
    };
    let (tx, rx) = (tx.clone(), rx.clone());
    cx.open_window(
        WindowOptions {
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(0.), px(0.)), size: canvas })),
            app_id: Some("todobar".into()),
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: (mode == CanvasMode::Toplevel).then_some(WindowDecorations::Client),
            window_min_size: (mode == CanvasMode::Toplevel).then(|| size(px(30.), px(96.))),
            kind,
            ..Default::default()
        },
        move |window, cx| {
            let receiver = rx.borrow_mut().take().expect("command receiver already consumed");
            cx.new(|cx| Root::new(canvas, mode, tx, receiver, window, cx))
        },
    )?;
    Ok(())
}

fn main() {
    let (tx, rx) = mpsc::channel::<Command>();
    claim_instance(tx.clone());
    let rx = Rc::new(RefCell::new(Some(rx)));

    application().with_assets(Assets).run(move |cx: &mut App| {
        ui::text_input::bind_keys(cx);
        ui::root::bind_keys(cx);

        let canvas = cx
            .primary_display()
            .map(|d| d.bounds().size)
            .unwrap_or_else(|| size(px(1920.), px(1080.)));
        if let Err(e) = open_root(cx, CanvasMode::Canvas, canvas, &tx, &rx) {
            eprintln!(
                "todobar: compositor does not support wlr-layer-shell (needed for always-on-top); falling back to a normal window ({e})"
            );
            // The first attempt may have consumed the receiver only if it got as far as building the view.
            if rx.borrow().is_none() {
                eprintln!("todobar: cannot fall back after view construction failed");
                cx.quit();
                return;
            }
            let saved = settings::Settings::load().expanded;
            let win = saved.map_or(size(px(290.), px(380.)), |r| size(px(r.w), px(r.h)));
            if let Err(e) = open_root(cx, CanvasMode::Toplevel, win, &tx, &rx) {
                eprintln!("todobar: could not open a window: {e}");
                cx.quit();
            }
        }
    });
}
