use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};

use super::sync::Remote;

const DEFAULT_BASE: &str = "https://tasks.googleapis.com/tasks/v1";

/// `TODOBAR_GOOGLE_API_BASE` redirects the API (used by the mock-server test).
fn base() -> String {
    std::env::var("TODOBAR_GOOGLE_API_BASE").unwrap_or_else(|_| DEFAULT_BASE.to_string())
}

fn err(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            anyhow!("Google Tasks API {code}: {}", resp.into_string().unwrap_or_default())
        }
        other => anyhow!("Google Tasks request failed: {other}"),
    }
}

#[derive(Deserialize)]
struct Lists {
    #[serde(default)]
    items: Vec<ListItem>,
    #[serde(rename = "nextPageToken")]
    next: Option<String>,
}

#[derive(Deserialize)]
struct ListItem {
    id: String,
    #[serde(default)]
    title: String,
}

pub fn lists(token: &str) -> Result<Vec<(String, String)>> {
    let mut out = vec![];
    let mut page: Option<String> = None;
    loop {
        let mut req = ureq::get(&format!("{}/users/@me/lists", base())).set("Authorization", &format!("Bearer {token}")).query("maxResults", "100");
        if let Some(p) = &page {
            req = req.query("pageToken", p);
        }
        let l: Lists = req.call().map_err(err)?.into_json()?;
        out.extend(l.items.into_iter().map(|i| (i.id, i.title)));
        match l.next {
            Some(n) => page = Some(n),
            None => return Ok(out),
        }
    }
}

pub fn create_list(token: &str, title: &str) -> Result<String> {
    let v: Value = ureq::post(&format!("{}/users/@me/lists", base()))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(json!({ "title": title }))
        .map_err(err)?
        .into_json()?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow!("no id in created list"))
}

pub fn tasks(token: &str, list: &str) -> Result<Vec<Remote>> {
    let mut out = vec![];
    let mut page: Option<String> = None;
    loop {
        let mut req = ureq::get(&format!("{}/lists/{list}/tasks", base()))
            .set("Authorization", &format!("Bearer {token}"))
            .query("showCompleted", "true")
            .query("showHidden", "true")
            .query("showDeleted", "true")
            .query("maxResults", "100");
        if let Some(p) = &page {
            req = req.query("pageToken", p);
        }
        let v: Value = req.call().map_err(err)?.into_json()?;
        for t in v["items"].as_array().cloned().unwrap_or_default() {
            out.push(parse_task(&t));
        }
        match v["nextPageToken"].as_str() {
            Some(n) => page = Some(n.to_string()),
            None => return Ok(out),
        }
    }
}

pub fn parse_task(t: &Value) -> Remote {
    Remote {
        id: t["id"].as_str().unwrap_or_default().to_string(),
        title: t["title"].as_str().unwrap_or_default().to_string(),
        notes: t["notes"].as_str().map(str::to_string),
        done: t["status"].as_str() == Some("completed"),
        // Google keeps only the date; the time part is always midnight UTC.
        date: t["due"].as_str().and_then(|d| d.get(..10)).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        deleted: t["deleted"].as_bool().unwrap_or(false),
    }
}

pub fn body(title: &str, notes: Option<&str>, done: bool, date: Option<NaiveDate>) -> Value {
    let mut v = json!({
        "title": title,
        "notes": notes,
        "status": if done { "completed" } else { "needsAction" },
        "due": date.map(|d| format!("{d}T00:00:00.000Z")),
    });
    if !done {
        v["completed"] = Value::Null; // reopening must clear the completion time
    }
    v
}

pub fn create_task(token: &str, list: &str, b: Value) -> Result<String> {
    let v: Value = ureq::post(&format!("{}/lists/{list}/tasks", base()))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(b)
        .map_err(err)?
        .into_json()?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow!("no id in created task"))
}

pub fn patch_task(token: &str, list: &str, task: &str, b: Value) -> Result<()> {
    ureq::request("PATCH", &format!("{}/lists/{list}/tasks/{task}", base()))
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(b)
        .map_err(err)?;
    Ok(())
}

pub fn delete_task(token: &str, list: &str, task: &str) -> Result<()> {
    match ureq::delete(&format!("{}/lists/{list}/tasks/{task}", base())).set("Authorization", &format!("Bearer {token}")).call() {
        Ok(_) | Err(ureq::Error::Status(404, _)) | Err(ureq::Error::Status(410, _)) => Ok(()),
        Err(e) => Err(err(e)),
    }
}
