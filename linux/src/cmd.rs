use uuid::Uuid;

use crate::google::sync::ListOutcome;
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
    GoogleConnected(Result<(), String>),
    GoogleLists(Result<Vec<(String, String)>, String>),
    /// A newly created Google list for this workspace.
    GoogleLinked(Uuid, Result<String, String>),
    SyncDone { outcomes: Vec<ListOutcome>, error: Option<String> },
}
