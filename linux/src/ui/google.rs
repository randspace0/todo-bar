//! Google Tasks: connect/disconnect, link a workspace to a list, background sync.

use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use gpui::Context;
use uuid::Uuid;

use crate::cmd::Command;
use crate::google::{api, auth, sync};
use crate::ui::root::{Overlay, Root};

const EDIT_DEBOUNCE: Duration = Duration::from_secs(5);
const PERIODIC: Duration = Duration::from_secs(300);

impl Root {
    pub fn refresh_google(&mut self) {
        self.google_creds =
            auth::Credentials::resolve(&self.settings.google_client_id, &self.settings.google_client_secret);
        self.google_connected = auth::is_connected();
    }

    pub fn current_list_linked(&self) -> bool {
        self.store.current_workspace().google_list_id.is_some()
    }

    fn any_linked(&self) -> bool {
        self.store.workspaces.iter().any(|w| w.google_list_id.is_some())
    }

    pub fn schedule_google_sync(&mut self) {
        if self.google_connected && self.any_linked() {
            self.sync_due = Some(Instant::now() + EDIT_DEBOUNCE);
        }
    }

    pub fn google_tick(&mut self, cx: &mut Context<Self>) {
        if !self.google_connected {
            return;
        }
        if self.sync_due.is_some_and(|t| Instant::now() >= t) || (self.any_linked() && self.last_sync.elapsed() > PERIODIC) {
            self.sync_now(cx);
        }
    }

    pub fn google_connect(&mut self, cx: &mut Context<Self>) {
        let Some(creds) = self.google_creds.clone() else { return };
        self.google_status = "Waiting for Google sign-in in your browser…".into();
        let tx = self.cmd_tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Command::GoogleConnected(auth::login(&creds).map_err(|e| e.to_string())));
        });
        cx.notify();
    }

    pub fn google_disconnect(&mut self, cx: &mut Context<Self>) {
        auth::disconnect();
        self.google_connected = false;
        self.google_status.clear();
        self.sync_due = None;
        cx.notify();
    }

    pub fn open_google_lists(&mut self, cx: &mut Context<Self>) {
        let Some(creds) = self.google_creds.clone() else { return };
        self.overlay = Some(Overlay::GoogleLists(None));
        let tx = self.cmd_tx.clone();
        thread::spawn(move || {
            let r = auth::access_token(&creds).and_then(|t| api::lists(&t)).map_err(|e| e.to_string());
            let _ = tx.send(Command::GoogleLists(r));
        });
        cx.notify();
    }

    pub fn link_existing_list(&mut self, list_id: String, cx: &mut Context<Self>) {
        let ws = self.store.current_workspace_id;
        self.overlay = None;
        self.edit_store(cx, |s| s.set_google_list(ws, Some(list_id)));
        self.sync_due = Some(Instant::now());
    }

    pub fn link_new_list(&mut self, cx: &mut Context<Self>) {
        let Some(creds) = self.google_creds.clone() else { return };
        let ws = self.store.current_workspace_id;
        let name = self.store.current_workspace().name.clone();
        self.overlay = None;
        self.google_status = "Creating Google list…".into();
        let tx = self.cmd_tx.clone();
        thread::spawn(move || {
            let r = auth::access_token(&creds).and_then(|t| api::create_list(&t, &name)).map_err(|e| e.to_string());
            let _ = tx.send(Command::GoogleLinked(ws, r));
        });
        cx.notify();
    }

    pub fn unlink_list(&mut self, cx: &mut Context<Self>) {
        let ws = self.store.current_workspace_id;
        if let Some(list) = self.store.current_workspace().google_list_id.clone() {
            self.sync_state.known.remove(&list);
            self.sync_state.save();
        }
        self.overlay = None;
        self.edit_store(cx, |s| s.set_google_list(ws, None));
    }

    pub fn sync_now(&mut self, cx: &mut Context<Self>) {
        self.sync_due = None;
        let Some(creds) = self.google_creds.clone() else { return };
        if self.syncing || !self.google_connected {
            return;
        }
        let inputs: Vec<sync::ListInput> = self
            .store
            .workspaces
            .iter()
            .filter_map(|w| {
                let list_id = w.google_list_id.clone()?;
                Some(sync::ListInput {
                    workspace: w.id,
                    known: self.sync_state.known.get(&list_id).map(|k| k.iter().cloned().collect()).unwrap_or_else(HashSet::new),
                    list_id,
                    items: self.store.all_items.iter().filter(|i| i.workspace_id == Some(w.id)).cloned().collect(),
                })
            })
            .collect();
        if inputs.is_empty() {
            return;
        }
        self.syncing = true;
        self.last_sync = Instant::now();
        self.google_status = "Syncing…".into();
        let tx = self.cmd_tx.clone();
        thread::spawn(move || {
            let mut outcomes = vec![];
            let mut error = None;
            match auth::access_token(&creds) {
                Err(e) => error = Some(e.to_string()),
                Ok(token) => {
                    for input in inputs {
                        match sync::sync_list(&token, input) {
                            Ok(o) => outcomes.push(o),
                            Err(e) => {
                                error = Some(e.to_string());
                                break;
                            }
                        }
                    }
                }
            }
            let _ = tx.send(Command::SyncDone { outcomes, error });
        });
        cx.notify();
    }

    pub fn handle_google_command(&mut self, cmd: Command, cx: &mut Context<Self>) {
        match cmd {
            Command::GoogleConnected(r) => match r {
                Ok(()) => {
                    self.google_connected = true;
                    self.google_status = "Connected to Google Tasks".into();
                    self.sync_due = self.any_linked().then(Instant::now);
                }
                Err(e) => self.google_status = format!("Google sign-in failed: {}", first_line(&e)),
            },
            Command::GoogleLists(r) => match r {
                Ok(lists) => self.overlay = Some(Overlay::GoogleLists(Some(lists))),
                Err(e) => {
                    self.overlay = None;
                    self.google_status = format!("Could not load lists: {}", first_line(&e));
                }
            },
            Command::GoogleLinked(ws, r) => match r {
                Ok(list_id) => {
                    self.edit_store(cx, |s| s.set_google_list(ws, Some(list_id)));
                    self.sync_due = Some(Instant::now());
                }
                Err(e) => self.google_status = format!("Could not create list: {}", first_line(&e)),
            },
            Command::SyncDone { outcomes, error } => {
                self.syncing = false;
                let now = Utc::now();
                for o in &outcomes {
                    self.store.apply_sync(o, now);
                    self.sync_state.known.insert(o.list_id.clone(), o.known.clone());
                }
                self.sync_state.save();
                self.needs_sync = true;
                self.google_status = match error {
                    Some(e) => format!("Sync failed: {}", first_line(&e)),
                    None => format!("Synced {}", Local::now().format("%H:%M")),
                };
            }
            _ => {}
        }
        cx.notify();
    }
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or(s);
    l.chars().take(80).collect()
}

#[allow(unused)]
fn _id(_: Uuid) {}
