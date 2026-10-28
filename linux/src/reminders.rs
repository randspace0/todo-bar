use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Duration as CDuration, Local, Utc};
use uuid::Uuid;

use crate::cmd::Command;
use crate::model::{format_time, local_at, TodoItem, Workspace};

pub struct Snapshot {
    pub items: Vec<TodoItem>,
    pub workspaces: Vec<Workspace>,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct Reminders {
    tx: Sender<Snapshot>,
}

impl Reminders {
    pub fn spawn(commands: Sender<Command>) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || run(rx, commands));
        Self { tx }
    }

    pub fn update(&self, snapshot: Snapshot) {
        let _ = self.tx.send(snapshot);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reminder {
    pub key: (Uuid, &'static str),
    pub task: Uuid,
    pub at: DateTime<Utc>,
    pub summary: String,
    pub body: String,
}

/// Same rules as the macOS scheduler: 15 min before and at a timed deadline, 9:00 on the
/// day for date-only ones; only future ones, soonest 60.
pub fn compute(items: &[TodoItem], workspaces: &[Workspace], now: DateTime<Utc>) -> Vec<Reminder> {
    let names: HashMap<Uuid, &str> = if workspaces.len() > 1 {
        workspaces.iter().map(|w| (w.id, w.name.as_str())).collect()
    } else {
        HashMap::new()
    };
    let mut out = vec![];
    for item in items.iter().filter(|i| !i.is_done) {
        let Some(deadline) = item.deadline else { continue };
        let summary = match item.workspace_id.and_then(|id| names.get(&id)) {
            Some(ws) => format!("{} — {}", item.title, ws),
            None => item.title.clone(),
        };
        let mut push = |suffix: &'static str, at: DateTime<Utc>, body: String| {
            out.push(Reminder { key: (item.id, suffix), task: item.id, at, summary: summary.clone(), body });
        };
        if item.deadline_has_time {
            let time = format_time(deadline.with_timezone(&Local));
            push("soon", deadline - CDuration::minutes(15), format!("Due in 15 minutes ({time})"));
            push("due", deadline, "Due now".into());
        } else {
            let day = deadline.with_timezone(&Local).date_naive();
            push("day", local_at(day, 9, 0), "Due today".into());
        }
    }
    out.retain(|r| r.at > now);
    out.sort_by_key(|r| r.at);
    out.truncate(60);
    out
}

fn run(rx: Receiver<Snapshot>, commands: Sender<Command>) {
    let mut pending: Vec<Reminder> = vec![];
    let mut fired: HashSet<(Uuid, &'static str)> = HashSet::new();
    let mut latest: Option<Snapshot> = None;
    loop {
        let wait = pending
            .first()
            .map(|r| (r.at - Utc::now()).to_std().unwrap_or(Duration::ZERO))
            .unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait) {
            Ok(snap) => {
                latest = Some(snap);
                // Debounce: wait for 500 ms of quiet.
                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(s) => latest = Some(s),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some(s) = latest.take() {
            pending = if s.enabled { compute(&s.items, &s.workspaces, Utc::now()) } else { vec![] };
            pending.retain(|r| !fired.contains(&r.key));
        }
        let now = Utc::now();
        while pending.first().is_some_and(|r| r.at <= now) {
            let r = pending.remove(0);
            fired.insert(r.key);
            fire(&r, commands.clone());
        }
    }
}

fn fire(r: &Reminder, commands: Sender<Command>) {
    let (task, summary, body) = (r.task, r.summary.clone(), r.body.clone());
    // The dbus handle isn't Send, so show and wait for the click on one dedicated thread.
    thread::spawn(move || {
        let shown = notify_rust::Notification::new()
            .appname("TodoBar")
            .summary(&summary)
            .body(&body)
            .icon("todobar")
            .action("default", "Open")
            .show();
        match shown {
            Ok(handle) => handle.wait_for_action(|action| {
                if action == "default" {
                    let _ = commands.send(Command::OpenTask(task));
                }
            }),
            Err(e) => eprintln!("todobar: notification failed: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn timed_gives_two_dateonly_gives_nine_am() {
        let ws = vec![Workspace::new("Todo")];
        let day = NaiveDate::from_ymd_opt(2030, 5, 1).unwrap();
        let mut timed = TodoItem::new("a".into());
        timed.deadline = Some(local_at(day, 14, 0));
        timed.deadline_has_time = true;
        let mut dateonly = TodoItem::new("b".into());
        dateonly.deadline = Some(local_at(day, 0, 0));
        let now = local_at(NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(), 0, 0);
        let r = compute(&[timed, dateonly], &ws, now);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].at, local_at(day, 9, 0));
        assert_eq!(r[1].body, "Due in 15 minutes (2:00 PM)");
        assert_eq!(r[1].summary, "a");
    }
}
