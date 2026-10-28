use uuid::Uuid;

use crate::settings::Theme;

/// Requests from background threads (tray, reminders, single-instance socket) to the UI thread.
#[derive(Debug, Clone)]
pub enum Command {
    Show,
    ToggleCollapsed,
    SwitchWorkspace(Uuid),
    NewWorkspace,
    SetTheme(Theme),
    ToggleLaunchAtLogin,
    OpenTask(Uuid),
    Quit,
}
