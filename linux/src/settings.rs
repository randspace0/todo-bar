use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];
    pub fn title(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowStyle {
    #[default]
    Glass,
    Standard,
}

impl WindowStyle {
    pub const ALL: [WindowStyle; 2] = [WindowStyle::Glass, WindowStyle::Standard];
    pub fn title(self) -> &'static str {
        match self {
            WindowStyle::Glass => "Liquid Glass",
            WindowStyle::Standard => "Standard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortMode {
    #[default]
    Manual,
    Deadline,
}

impl SortMode {
    pub const ALL: [SortMode; 2] = [SortMode::Manual, SortMode::Deadline];
    pub fn title(self) -> &'static str {
        match self {
            SortMode::Manual => "Manual",
            SortMode::Deadline => "By Deadline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub window_style: WindowStyle,
    #[serde(default = "yes")]
    pub reminders_enabled: bool,
    #[serde(default = "yes")]
    pub fade_when_idle: bool,
    #[serde(default = "yes")]
    pub show_done: bool,
    pub sort_mode: SortMode,
    pub expanded: Option<SavedRect>,
    pub is_collapsed: bool,
    pub edge: Edge,
    pub tab_y: f32,
    pub login_item_configured: bool,
    /// Google OAuth desktop client (env TODOBAR_GOOGLE_CLIENT_ID / _SECRET override these).
    pub google_client_id: String,
    pub google_client_secret: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            window_style: WindowStyle::Glass,
            reminders_enabled: true,
            fade_when_idle: true,
            show_done: true,
            sort_mode: SortMode::Manual,
            expanded: None,
            is_collapsed: false,
            edge: Edge::Right,
            tab_y: 20.0,
            login_item_configured: false,
            google_client_id: String::new(),
            google_client_secret: String::new(),
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("TodoBar").join("settings.json")
    }

    pub fn load() -> Self {
        fs::read(Self::path())
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, data).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }
}
