use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::model::*;

#[derive(Clone)]
struct Snapshot {
    items: Vec<TodoItem>,
    workspaces: Vec<Workspace>,
    current: Uuid,
}

const UNDO_CAP: usize = 200;

pub struct Store {
    pub all_items: Vec<TodoItem>,
    pub workspaces: Vec<Workspace>,
    pub current_workspace_id: Uuid,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag_undo_pending: bool,
    tasks_path: Option<PathBuf>,
    workspaces_path: Option<PathBuf>,
}

impl Store {
    /// Loads from `<data_dir>/TodoBar/`.
    pub fn load() -> Self {
        let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("TodoBar");
        let _ = fs::create_dir_all(&dir);
        let mut store = Self::empty();
        store.tasks_path = Some(dir.join("tasks.json"));
        store.workspaces_path = Some(dir.join("workspaces.json"));
        store.read_files();
        store
    }

    fn empty() -> Self {
        let ws = Workspace { id: Uuid::new_v4(), name: "Todo".into() };
        Self {
            all_items: vec![],
            current_workspace_id: ws.id,
            workspaces: vec![ws],
            undo: vec![],
            redo: vec![],
            drag_undo_pending: false,
            tasks_path: None,
            workspaces_path: None,
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::empty()
    }

    fn read_files(&mut self) {
        if let Some(path) = &self.tasks_path {
            if let Ok(data) = fs::read(path) {
                match serde_json::from_slice::<Vec<TodoItem>>(&data) {
                    Ok(items) => self.all_items = items,
                    Err(e) => {
                        eprintln!("todobar: {} unreadable ({e}); moved to tasks.json.bak", path.display());
                        let _ = fs::rename(path, path.with_extension("json.bak"));
                    }
                }
            }
        }
        let mut loaded = false;
        if let Some(path) = &self.workspaces_path {
            if let Ok(data) = fs::read(path) {
                if let Ok(file) = serde_json::from_slice::<WorkspacesFile>(&data) {
                    if !file.workspaces.is_empty() {
                        self.current_workspace_id =
                            if file.workspaces.iter().any(|w| w.id == file.current_id) {
                                file.current_id
                            } else {
                                file.workspaces[0].id
                            };
                        self.workspaces = file.workspaces;
                        loaded = true;
                    }
                }
            }
        }
        if !loaded {
            self.save_workspaces();
        }
        let first = self.workspaces[0].id;
        let mut adopted = false;
        for item in &mut self.all_items {
            if !item.workspace_id.is_some_and(|id| self.workspaces.iter().any(|w| w.id == id)) {
                item.workspace_id = Some(first);
                adopted = true;
            }
        }
        if adopted {
            self.save();
        }
    }

    // MARK: current workspace

    pub fn items(&self) -> Vec<TodoItem> {
        self.all_items
            .iter()
            .filter(|i| i.workspace_id == Some(self.current_workspace_id))
            .cloned()
            .collect()
    }

    pub fn current_workspace(&self) -> &Workspace {
        self.workspaces
            .iter()
            .find(|w| w.id == self.current_workspace_id)
            .unwrap_or(&self.workspaces[0])
    }

    pub fn open_count(&self) -> usize {
        self.items().iter().filter(|i| !i.is_done).count()
    }

    pub fn done_count(&self) -> usize {
        self.items().iter().filter(|i| i.is_done).count()
    }

    pub fn overdue_count(&self, now: DateTime<Utc>) -> usize {
        self.items().iter().filter(|i| i.is_overdue(now)).count()
    }

    pub fn task_count(&self, ws: Uuid) -> usize {
        self.all_items.iter().filter(|i| i.workspace_id == Some(ws)).count()
    }

    pub fn item(&self, id: Uuid) -> Option<&TodoItem> {
        self.all_items.iter().find(|i| i.id == id)
    }

    // MARK: workspaces

    pub fn switch_workspace(&mut self, id: Uuid) {
        if id != self.current_workspace_id && self.workspaces.iter().any(|w| w.id == id) {
            self.current_workspace_id = id;
            self.save_workspaces();
        }
    }

    pub fn add_workspace(&mut self, name: &str) -> Uuid {
        self.record_undo();
        let ws = Workspace { id: Uuid::new_v4(), name: name.to_string() };
        let id = ws.id;
        self.workspaces.push(ws);
        self.current_workspace_id = id;
        self.save_workspaces();
        id
    }

    pub fn rename_workspace(&mut self, id: Uuid, name: &str) {
        let trimmed = name.trim();
        let Some(index) = self.workspaces.iter().position(|w| w.id == id) else { return };
        if trimmed.is_empty() || self.workspaces[index].name == trimmed {
            return;
        }
        self.record_undo();
        self.workspaces[index].name = trimmed.to_string();
        self.save_workspaces();
    }

    pub fn delete_workspace(&mut self, id: Uuid) {
        if self.workspaces.len() < 2 {
            return;
        }
        self.record_undo();
        self.all_items.retain(|i| i.workspace_id != Some(id));
        self.workspaces.retain(|w| w.id != id);
        if self.current_workspace_id == id {
            self.current_workspace_id = self.workspaces[0].id;
        }
        self.save();
        self.save_workspaces();
    }

    pub fn move_task_to_workspace(&mut self, id: Uuid, ws: Uuid) {
        let Some(index) = self.all_items.iter().position(|i| i.id == id) else { return };
        if self.all_items[index].workspace_id == Some(ws) {
            return;
        }
        self.record_undo();
        let mut item = self.all_items.remove(index);
        item.workspace_id = Some(ws);
        self.all_items.push(item);
        self.save();
    }

    // MARK: tasks

    pub fn add(&mut self, title: &str, deadline: Option<DateTime<Utc>>, has_time: bool, repeat: Option<RepeatRule>) {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return;
        }
        self.record_undo();
        let mut item = TodoItem::new(trimmed.to_string());
        item.deadline = deadline;
        item.deadline_has_time = deadline.is_some() && has_time;
        item.repeat_rule = if deadline.is_none() { None } else { repeat };
        item.workspace_id = Some(self.current_workspace_id);
        self.all_items.push(item);
        self.save();
    }

    pub fn toggle(&mut self, id: Uuid) {
        self.toggle_at(id, Utc::now());
    }

    pub fn toggle_at(&mut self, id: Uuid, now: DateTime<Utc>) {
        let Some(index) = self.all_items.iter().position(|i| i.id == id) else { return };
        self.record_undo();
        let mut item = self.all_items[index].clone();
        item.is_done = !item.is_done;
        item.completed_at = if item.is_done { Some(trunc(now)) } else { None };

        if let (true, Some(rule)) = (item.is_done, item.repeat_rule) {
            let base = item.deadline.unwrap_or_else(|| {
                local_start_of_day(now.with_timezone(&chrono::Local).date_naive())
            });
            let mut next = TodoItem::new(item.title.clone());
            next.deadline = Some(rule.next(base, item.deadline_has_time, now));
            next.deadline_has_time = item.deadline_has_time;
            next.repeat_rule = Some(rule);
            next.notes = item.notes.clone();
            next.workspace_id = item.workspace_id;
            item.repeat_rule = None;
            self.all_items[index] = item;
            self.all_items.insert(index + 1, next);
        } else {
            self.all_items[index] = item;
        }
        self.save();
    }

    pub fn edit(&mut self, id: Uuid, title: &str, notes: &str) {
        let t = title.trim();
        let n = notes.trim();
        let Some(cur) = self.item(id) else { return };
        if t.is_empty() || (cur.title == t && cur.notes.as_deref().unwrap_or("") == n) {
            return;
        }
        self.record_undo();
        self.mutate(id, |i| {
            i.title = t.to_string();
            i.notes = if n.is_empty() { None } else { Some(n.to_string()) };
        });
    }

    pub fn set_deadline(&mut self, id: Uuid, date: Option<DateTime<Utc>>, has_time: bool, repeat: Option<RepeatRule>) {
        self.record_undo();
        self.mutate(id, |i| {
            i.deadline = date;
            i.deadline_has_time = date.is_some() && has_time;
            i.repeat_rule = if date.is_none() { None } else { repeat };
        });
    }

    pub fn move_(&mut self, id: Uuid, to: Uuid) {
        if !self.can_move(id, to) {
            return;
        }
        self.record_undo();
        self.reorder(id, to);
    }

    pub fn begin_drag(&mut self) {
        self.drag_undo_pending = true;
    }

    pub fn move_during_drag(&mut self, id: Uuid, to: Uuid) {
        if !self.can_move(id, to) {
            return;
        }
        if self.drag_undo_pending {
            self.record_undo();
            self.drag_undo_pending = false;
        }
        self.reorder(id, to);
    }

    pub fn delete(&mut self, id: Uuid) {
        self.record_undo();
        self.all_items.retain(|i| i.id != id);
        self.save();
    }

    pub fn clear_completed(&mut self) {
        if self.done_count() == 0 {
            return;
        }
        self.record_undo();
        let ws = self.current_workspace_id;
        self.all_items.retain(|i| !(i.is_done && i.workspace_id == Some(ws)));
        self.save();
    }

    // MARK: undo / redo

    pub fn undo(&mut self) {
        if let Some(snap) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snap);
        }
    }

    pub fn redo(&mut self) {
        if let Some(snap) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snap);
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            items: self.all_items.clone(),
            workspaces: self.workspaces.clone(),
            current: self.current_workspace_id,
        }
    }

    fn record_undo(&mut self) {
        let snap = self.snapshot();
        self.undo.push(snap);
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, snap: Snapshot) {
        self.all_items = snap.items;
        self.workspaces = snap.workspaces;
        self.current_workspace_id = snap.current;
        self.save();
        self.save_workspaces();
    }

    // MARK: private

    fn can_move(&self, id: Uuid, to: Uuid) -> bool {
        id != to && self.item(id).is_some() && self.item(to).is_some()
    }

    fn reorder(&mut self, id: Uuid, to: Uuid) {
        let (Some(from), Some(dest)) = (
            self.all_items.iter().position(|i| i.id == id),
            self.all_items.iter().position(|i| i.id == to),
        ) else {
            return;
        };
        let item = self.all_items.remove(from);
        self.all_items.insert(dest, item);
        self.save();
    }

    fn mutate(&mut self, id: Uuid, f: impl FnOnce(&mut TodoItem)) {
        if let Some(i) = self.all_items.iter_mut().find(|i| i.id == id) {
            f(i);
            self.save();
        }
    }

    fn write_atomic(path: &PathBuf, data: &[u8]) {
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, data).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }

    fn save(&self) {
        if let (Some(path), Ok(data)) = (&self.tasks_path, serde_json::to_vec_pretty(&self.all_items)) {
            Self::write_atomic(path, &data);
        }
    }

    fn save_workspaces(&self) {
        let file = WorkspacesFile { workspaces: self.workspaces.clone(), current_id: self.current_workspace_id };
        if let (Some(path), Ok(data)) = (&self.workspaces_path, serde_json::to_vec_pretty(&file)) {
            Self::write_atomic(path, &data);
        }
    }
}

fn trunc(d: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp(d.timestamp(), 0).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn toggle_repeating_adds_copy_and_clears_rule() {
        let mut s = Store::in_memory();
        let d = local_at(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(), 9, 0);
        s.add("water", Some(d), true, Some(RepeatRule::Daily));
        let id = s.all_items[0].id;
        s.toggle_at(id, local_at(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(), 10, 0));
        assert_eq!(s.all_items.len(), 2);
        assert!(s.all_items[0].is_done && s.all_items[0].repeat_rule.is_none());
        assert_eq!(s.all_items[1].repeat_rule, Some(RepeatRule::Daily));
        assert!(!s.all_items[1].is_done);
        assert_eq!(s.all_items[1].workspace_id, s.all_items[0].workspace_id);
    }

    #[test]
    fn undo_redo_restores_items_and_workspace() {
        let mut s = Store::in_memory();
        s.add("a", None, false, None);
        let first = s.current_workspace_id;
        let ws = s.add_workspace("Two");
        assert_eq!(s.current_workspace_id, ws);
        s.undo();
        assert_eq!(s.current_workspace_id, first);
        assert_eq!(s.workspaces.len(), 1);
        s.redo();
        assert_eq!(s.current_workspace_id, ws);
        s.undo();
        s.undo();
        assert!(s.all_items.is_empty());
    }

    #[test]
    fn drag_is_single_undo_step() {
        let mut s = Store::in_memory();
        for t in ["a", "b", "c"] {
            s.add(t, None, false, None);
        }
        let (a, b, c) = (s.all_items[0].id, s.all_items[1].id, s.all_items[2].id);
        s.begin_drag();
        s.move_during_drag(a, b);
        s.move_during_drag(a, c);
        assert_eq!(s.all_items[2].id, a);
        s.undo();
        assert_eq!(s.all_items[0].id, a);
    }

    #[test]
    fn cannot_delete_last_workspace() {
        let mut s = Store::in_memory();
        let id = s.current_workspace_id;
        s.delete_workspace(id);
        assert_eq!(s.workspaces.len(), 1);
    }
}
