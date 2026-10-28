use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::api;
use crate::model::TodoItem;

/// A Google task, reduced to the fields TodoBar syncs.
#[derive(Clone, Debug, PartialEq)]
pub struct Remote {
    pub id: String,
    pub title: String,
    pub notes: Option<String>,
    pub done: bool,
    pub date: Option<NaiveDate>,
    pub deleted: bool,
}

/// FNV-1a: stable across Rust versions, unlike `DefaultHasher`, because it is persisted.
pub fn hash_content(title: &str, notes: Option<&str>, done: bool, date: Option<NaiveDate>) -> u64 {
    let notes = notes.map(str::trim).filter(|n| !n.is_empty()).unwrap_or("");
    let text = format!("{}\0{}\0{}\0{}", title.trim(), notes, done, date.map(|d| d.to_string()).unwrap_or_default());
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn local_date(i: &TodoItem) -> Option<NaiveDate> {
    i.deadline.map(|d| d.with_timezone(&Local).date_naive())
}

pub fn local_hash(i: &TodoItem) -> u64 {
    hash_content(&i.title, i.notes.as_deref(), i.is_done, local_date(i))
}

pub fn remote_hash(r: &Remote) -> u64 {
    hash_content(&r.title, r.notes.as_deref(), r.done, r.date)
}

#[derive(Debug, PartialEq)]
pub enum Op {
    CreateRemote(Uuid),
    UpdateRemote(Uuid, String),
    DeleteRemote(String),
    CreateLocal(Remote),
    UpdateLocal(Uuid, Remote),
    DeleteLocal(Uuid),
}

/// Decides what to do for one list. `known` = Google ids that both sides had after the previous
/// sync: a known id with no local item was deleted locally; a linked local item whose remote is
/// gone was deleted on Google. If both sides changed, local wins.
pub fn reconcile(local: &[TodoItem], remote: &[Remote], known: &HashSet<String>) -> Vec<Op> {
    let remote_by_id: HashMap<&str, &Remote> = remote.iter().map(|r| (r.id.as_str(), r)).collect();
    let local_ids: HashSet<&str> = local.iter().filter_map(|i| i.google_id.as_deref()).collect();
    let mut ops = vec![];

    for item in local {
        let Some(gid) = item.google_id.as_deref() else {
            ops.push(Op::CreateRemote(item.id));
            continue;
        };
        let live = remote_by_id.get(gid).filter(|r| !r.deleted);
        let changed_locally = item.google_hash != Some(local_hash(item));
        match live {
            None if changed_locally => ops.push(Op::CreateRemote(item.id)),
            None => ops.push(Op::DeleteLocal(item.id)),
            Some(r) => {
                if changed_locally {
                    if remote_hash(r) != local_hash(item) {
                        ops.push(Op::UpdateRemote(item.id, gid.to_string()));
                    }
                } else if item.google_hash != Some(remote_hash(r)) {
                    ops.push(Op::UpdateLocal(item.id, (*r).clone()));
                }
            }
        }
    }
    for r in remote {
        if r.deleted || local_ids.contains(r.id.as_str()) {
            continue;
        }
        if known.contains(&r.id) {
            ops.push(Op::DeleteRemote(r.id.clone()));
        } else if !r.title.trim().is_empty() {
            ops.push(Op::CreateLocal(r.clone()));
        }
    }
    ops
}

/// What the UI thread must apply to its store after a list was synced.
#[derive(Clone, Debug)]
pub struct ListOutcome {
    pub workspace: Uuid,
    pub list_id: String,
    /// (local id, google id, hash of the content now on both sides)
    pub links: Vec<(Uuid, String, u64)>,
    /// (local id, local hash the update was computed from, remote task)
    pub updates: Vec<(Uuid, u64, Remote)>,
    pub creates: Vec<Remote>,
    /// (local id, local hash the delete was decided on)
    pub deletes: Vec<(Uuid, u64)>,
    /// Google ids present on both sides after this sync.
    pub known: Vec<String>,
}

pub struct ListInput {
    pub workspace: Uuid,
    pub list_id: String,
    pub items: Vec<TodoItem>,
    pub known: HashSet<String>,
}

pub fn sync_list(token: &str, input: ListInput) -> Result<ListOutcome> {
    let remote = api::tasks(token, &input.list_id)?;
    let ops = reconcile(&input.items, &remote, &input.known);
    let by_id: HashMap<Uuid, &TodoItem> = input.items.iter().map(|i| (i.id, i)).collect();
    let mut out = ListOutcome {
        workspace: input.workspace,
        list_id: input.list_id.clone(),
        links: vec![],
        updates: vec![],
        creates: vec![],
        deletes: vec![],
        known: vec![],
    };
    // Ids both sides share after this sync; starts as the linked items whose remote is still live.
    let live: HashSet<&str> = remote.iter().filter(|r| !r.deleted).map(|r| r.id.as_str()).collect();
    let mut known: HashSet<String> =
        input.items.iter().filter_map(|i| i.google_id.clone()).filter(|g| live.contains(g.as_str())).collect();

    for op in ops {
        match op {
            Op::CreateRemote(id) => {
                let i = by_id[&id];
                let gid = api::create_task(
                    token,
                    &input.list_id,
                    api::body(&i.title, i.notes.as_deref(), i.is_done, local_date(i)),
                )?;
                known.insert(gid.clone());
                out.links.push((id, gid, local_hash(i)));
            }
            Op::UpdateRemote(id, gid) => {
                let i = by_id[&id];
                api::patch_task(token, &input.list_id, &gid, api::body(&i.title, i.notes.as_deref(), i.is_done, local_date(i)))?;
                out.links.push((id, gid, local_hash(i)));
            }
            Op::DeleteRemote(gid) => api::delete_task(token, &input.list_id, &gid)?,
            Op::CreateLocal(r) => {
                known.insert(r.id.clone());
                out.creates.push(r);
            }
            Op::UpdateLocal(id, r) => out.updates.push((id, local_hash(by_id[&id]), r)),
            Op::DeleteLocal(id) => {
                known.retain(|k| Some(k.as_str()) != by_id[&id].google_id.as_deref());
                out.deletes.push((id, local_hash(by_id[&id])));
            }
        }
    }
    out.known = known.into_iter().collect();
    Ok(out)
}

/// Google ids both sides had after each list's last sync, persisted next to the task data.
#[derive(Default, Serialize, Deserialize)]
pub struct SyncState {
    pub known: HashMap<String, Vec<String>>,
}

impl SyncState {
    fn path() -> PathBuf {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("TodoBar").join("google-sync.json")
    }

    pub fn load() -> Self {
        fs::read(Self::path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(Self::path(), data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked(title: &str, gid: &str, done: bool) -> TodoItem {
        let mut i = TodoItem::new(title.into());
        i.is_done = done;
        i.google_id = Some(gid.into());
        i.google_hash = Some(local_hash(&i));
        i
    }

    fn remote(id: &str, title: &str, done: bool) -> Remote {
        Remote { id: id.into(), title: title.into(), notes: None, done, date: None, deleted: false }
    }

    fn known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn new_local_is_created_remote_and_new_remote_is_created_local() {
        let local = TodoItem::new("a".into());
        let ops = reconcile(&[local.clone()], &[remote("r1", "b", false)], &known(&[]));
        assert_eq!(ops, vec![Op::CreateRemote(local.id), Op::CreateLocal(remote("r1", "b", false))]);
    }

    #[test]
    fn remote_edit_pulls_local_edit_pushes_both_local_wins() {
        let item = linked("a", "r1", false);
        let k = known(&["r1"]);
        assert_eq!(reconcile(&[item.clone()], &[remote("r1", "a", true)], &k), vec![Op::UpdateLocal(item.id, remote("r1", "a", true))]);
        let mut edited = item.clone();
        edited.title = "a2".into();
        assert_eq!(reconcile(&[edited.clone()], &[remote("r1", "a", false)], &k), vec![Op::UpdateRemote(edited.id, "r1".into())]);
        assert_eq!(reconcile(&[edited.clone()], &[remote("r1", "other", false)], &k), vec![Op::UpdateRemote(edited.id, "r1".into())]);
        assert!(reconcile(&[item], &[remote("r1", "a", false)], &k).is_empty());
    }

    #[test]
    fn deletions_propagate_both_ways() {
        let item = linked("a", "r1", false);
        // Deleted on Google (absent or flagged), unchanged locally -> delete locally.
        assert_eq!(reconcile(&[item.clone()], &[], &known(&["r1"])), vec![Op::DeleteLocal(item.id)]);
        let mut gone = remote("r1", "a", false);
        gone.deleted = true;
        assert_eq!(reconcile(&[item.clone()], &[gone.clone()], &known(&["r1"])), vec![Op::DeleteLocal(item.id)]);
        // Edited locally meanwhile -> re-create instead of losing the edit.
        let mut edited = item.clone();
        edited.title = "keep me".into();
        assert_eq!(reconcile(&[edited.clone()], &[gone], &known(&["r1"])), vec![Op::CreateRemote(edited.id)]);
        // Deleted locally (known id, no local item) -> delete on Google.
        assert_eq!(reconcile(&[], &[remote("r1", "a", false)], &known(&["r1"])), vec![Op::DeleteRemote("r1".into())]);
    }

    #[test]
    fn untitled_remote_is_not_imported_and_time_of_day_is_ignored_in_hash() {
        assert!(reconcile(&[], &[remote("r1", "  ", false)], &known(&[])).is_empty());
        let d = NaiveDate::from_ymd_opt(2026, 3, 1);
        assert_eq!(hash_content("t", Some(" "), false, d), hash_content("t", None, false, d));
        assert_ne!(hash_content("t", None, false, d), hash_content("t", None, true, d));
    }

    // End-to-end against an in-process fake of the Tasks REST API.
    mod mock {
        use super::*;
        use serde_json::{json, Value};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        pub type Tasks = Arc<Mutex<Vec<Value>>>;

        pub fn serve() -> (String, Tasks) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let tasks: Tasks = Arc::new(Mutex::new(vec![]));
            let state = tasks.clone();
            std::thread::spawn(move || {
                let mut n = 0;
                for mut s in listener.incoming().flatten() {
                    let mut buf = vec![0u8; 65536];
                    let mut len = 0;
                    // Read until the full body (per Content-Length) has arrived.
                    loop {
                        len += s.read(&mut buf[len..]).unwrap_or(0);
                        let text = String::from_utf8_lossy(&buf[..len]).to_string();
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let want = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                            if body.len() >= want {
                                break;
                            }
                        }
                        if len == 0 {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&buf[..len]).to_string();
                    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                    let mut first = head.lines().next().unwrap().split_whitespace();
                    let (method, path) = (first.next().unwrap(), first.next().unwrap());
                    let path = path.split('?').next().unwrap();
                    let id = path.rsplit('/').next().unwrap().to_string();
                    let mut tasks = state.lock().unwrap();
                    let reply: Value = match method {
                        "GET" => json!({ "items": *tasks }),
                        "POST" => {
                            n += 1;
                            let mut v: Value = serde_json::from_str(body).unwrap();
                            v["id"] = json!(format!("g{n}"));
                            tasks.push(v.clone());
                            v
                        }
                        "PATCH" => {
                            let patch: Value = serde_json::from_str(body).unwrap();
                            let t = tasks.iter_mut().find(|t| t["id"] == id.as_str()).unwrap();
                            for (k, v) in patch.as_object().unwrap() {
                                t[k] = v.clone();
                            }
                            t.clone()
                        }
                        "DELETE" => {
                            tasks.retain(|t| t["id"] != id.as_str());
                            json!({})
                        }
                        _ => json!({}),
                    };
                    let body = reply.to_string();
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            });
            (base, tasks)
        }
    }

    #[test]
    fn full_sync_round_trip_against_mock_api() {
        use crate::store::Store;
        use chrono::Utc;
        use serde_json::json;

        let (base, remote) = mock::serve();
        std::env::set_var("TODOBAR_GOOGLE_API_BASE", &base);
        let mut store = Store::in_memory();
        let ws = store.current_workspace_id;
        store.set_google_list(ws, Some("L".into()));
        store.add("local task", None, false, None);
        remote.lock().unwrap().push(json!({"id": "r1", "title": "from google", "status": "completed", "due": "2026-05-04T00:00:00.000Z", "notes": "n"}));
        let mut known: HashSet<String> = HashSet::new();
        let mut run = |store: &mut Store, known: &mut HashSet<String>| {
            let items = store.all_items.clone();
            let o = sync_list("tok", ListInput { workspace: ws, list_id: "L".into(), items, known: known.clone() }).unwrap();
            store.apply_sync(&o, Utc::now());
            *known = o.known.iter().cloned().collect();
        };

        // 1: local task is pushed, Google task is pulled (done, date, notes).
        run(&mut store, &mut known);
        assert_eq!(remote.lock().unwrap().len(), 2);
        let items = store.items();
        assert_eq!(items.len(), 2);
        let pulled = items.iter().find(|i| i.title == "from google").unwrap();
        assert!(pulled.is_done && pulled.notes.as_deref() == Some("n"));
        assert_eq!(local_date(pulled), NaiveDate::from_ymd_opt(2026, 5, 4));
        let local = items.iter().find(|i| i.title == "local task").unwrap().clone();
        assert!(local.google_id.is_some());

        // 2: no changes -> nothing to do.
        let before = remote.lock().unwrap().clone();
        run(&mut store, &mut known);
        assert_eq!(*remote.lock().unwrap(), before);

        // 3: edit local (pushes), edit remote (pulls) and reopen it.
        store.edit(local.id, "local task v2", "");
        for t in remote.lock().unwrap().iter_mut().filter(|t| t["id"] == "r1") {
            t["title"] = json!("google v2");
            t["status"] = json!("needsAction");
        }
        run(&mut store, &mut known);
        assert!(remote.lock().unwrap().iter().any(|t| t["title"] == "local task v2"));
        let pulled = store.item(pulled.id).unwrap();
        assert_eq!(pulled.title, "google v2");
        assert!(!pulled.is_done);

        // 4: delete locally -> removed from Google; delete on Google -> removed locally.
        store.delete(local.id);
        remote.lock().unwrap().retain(|t| t["id"] != "r1");
        run(&mut store, &mut known);
        assert!(remote.lock().unwrap().is_empty());
        assert!(store.items().is_empty());

        // 5: add -> edit -> sync -> undo -> sync keeps the same Google task (no delete/re-create).
        store.add("undo me", None, false, None);
        let id = store.items()[0].id;
        store.edit(id, "undo me v2", "");
        run(&mut store, &mut known);
        let gid = remote.lock().unwrap()[0]["id"].clone();
        store.undo();
        assert_eq!(store.item(id).unwrap().title, "undo me");
        assert_eq!(store.item(id).unwrap().google_id.as_deref(), gid.as_str());
        run(&mut store, &mut known);
        let tasks = remote.lock().unwrap().clone();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], gid);
        assert_eq!(tasks[0]["title"], "undo me");
    }
}
