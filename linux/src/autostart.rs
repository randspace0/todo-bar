use std::fs;
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("todobar.desktop"))
}

pub fn is_enabled() -> bool {
    path().is_some_and(|p| p.exists())
}

pub fn set(enabled: bool) {
    let Some(path) = path() else { return };
    if !enabled {
        let _ = fs::remove_file(&path);
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=TodoBar\nComment=Always-on-top todo list\nExec={}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
        exe.display()
    );
    if let Err(e) = fs::write(&path, entry) {
        eprintln!("todobar: could not write autostart entry: {e}");
    }
}
