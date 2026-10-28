use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RFC3339 with seconds precision, matching Swift's `.iso8601` strategy.
pub mod iso {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.to_rfc3339_opts(SecondsFormat::Secs, true)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        match s {
            Some(s) => DateTime::parse_from_rfc3339(&s)
                .map(|d| Some(d.with_timezone(&Utc)))
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

mod iso_required {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatRule {
    Daily,
    Weekdays,
    Weekly,
    Monthly,
}

impl RepeatRule {
    pub const ALL: [RepeatRule; 4] = [Self::Daily, Self::Weekdays, Self::Weekly, Self::Monthly];

    pub fn title(self) -> &'static str {
        match self {
            Self::Daily => "Every day",
            Self::Weekdays => "Every weekday",
            Self::Weekly => "Every week",
            Self::Monthly => "Every month",
        }
    }

    /// Next occurrence after `date` that is also in the future, so a task ticked off late
    /// doesn't come back already overdue.
    pub fn next(self, after: DateTime<Utc>, has_time: bool, now: DateTime<Utc>) -> DateTime<Utc> {
        let mut steps = 1;
        let mut next = self.occurrence(steps, after);
        loop {
            let past = if has_time {
                next <= now
            } else {
                next.with_timezone(&Local).date_naive() <= now.with_timezone(&Local).date_naive()
            };
            if !past {
                return next;
            }
            steps += 1;
            next = self.occurrence(steps, after);
        }
    }

    /// n-th occurrence counted from the original date, so "monthly on the 31st" doesn't drift.
    fn occurrence(self, n: u32, date: DateTime<Utc>) -> DateTime<Utc> {
        let local = date.with_timezone(&Local).naive_local();
        let out = match self {
            Self::Daily => local + Duration::days(n as i64),
            Self::Weekly => local + Duration::weeks(n as i64),
            Self::Monthly => local.checked_add_months(Months::new(n)).unwrap_or(local),
            Self::Weekdays => {
                let mut day = local;
                for _ in 0..n {
                    loop {
                        day += Duration::days(1);
                        if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                            break;
                        }
                    }
                }
                day
            }
        };
        local_to_utc(out)
    }
}

pub fn local_to_utc(naive: chrono::NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
}

pub fn local_start_of_day(date: NaiveDate) -> DateTime<Utc> {
    local_to_utc(date.and_hms_opt(0, 0, 0).unwrap())
}

pub fn local_at(date: NaiveDate, hour: u32, minute: u32) -> DateTime<Utc> {
    local_to_utc(date.and_hms_opt(hour, minute, 0).unwrap())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub is_done: bool,
    #[serde(default, with = "iso", skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deadline_has_time: bool,
    #[serde(with = "iso_required", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_rule: Option<RepeatRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, with = "iso", skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(rename = "workspaceID", default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
}

impl TodoItem {
    pub fn new(title: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title,
            is_done: false,
            deadline: None,
            deadline_has_time: false,
            created_at: DateTime::from_timestamp(now.timestamp(), 0).unwrap_or(now),
            repeat_rule: None,
            notes: None,
            completed_at: None,
            workspace_id: None,
        }
    }

    pub fn is_overdue(&self, now: DateTime<Utc>) -> bool {
        let Some(deadline) = self.deadline else { return false };
        if self.is_done {
            return false;
        }
        if self.deadline_has_time {
            return deadline < now;
        }
        deadline.with_timezone(&Local).date_naive() < now.with_timezone(&Local).date_naive()
    }

    pub fn is_due_today(&self, now: DateTime<Utc>) -> bool {
        let Some(deadline) = self.deadline else { return false };
        !self.is_done
            && deadline.with_timezone(&Local).date_naive() == now.with_timezone(&Local).date_naive()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspacesFile {
    pub workspaces: Vec<Workspace>,
    #[serde(rename = "currentID")]
    pub current_id: Uuid,
}

pub fn format_time(d: DateTime<Local>) -> String {
    let h = d.hour12().1;
    format!("{}:{:02} {}", h, d.minute(), if d.hour12().0 { "PM" } else { "AM" })
}

pub fn deadline_label(date: DateTime<Utc>, has_time: bool, now: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    let today = now.with_timezone(&Local).date_naive();
    let day = local.date_naive();
    let time = if has_time { format!(" {}", format_time(local)) } else { String::new() };
    let diff = (day - today).num_days();
    match diff {
        0 => return format!("Today{time}"),
        1 => return format!("Tomorrow{time}"),
        -1 => return format!("Yesterday{time}"),
        2..=6 => return format!("{}{time}", local.format("%A")),
        _ => {}
    }
    if day.year() == today.year() {
        format!("{}{time}", local.format("%-d %b"))
    } else {
        format!("{}{time}", local.format("%-d %b %Y"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        local_at(NaiveDate::from_ymd_opt(y, m, d).unwrap(), h, 0)
    }

    #[test]
    fn monthly_does_not_drift() {
        let jan31 = local(2026, 1, 31, 9);
        let feb = RepeatRule::Monthly.occurrence(1, jan31).with_timezone(&Local);
        assert_eq!((feb.month(), feb.day()), (2, 28));
        let mar = RepeatRule::Monthly.occurrence(2, jan31).with_timezone(&Local);
        assert_eq!((mar.month(), mar.day()), (3, 31));
    }

    #[test]
    fn weekdays_skip_weekend() {
        let fri = local(2026, 1, 2, 9); // Friday
        let next = RepeatRule::Weekdays.occurrence(1, fri).with_timezone(&Local);
        assert_eq!(next.weekday(), Weekday::Mon);
    }

    #[test]
    fn late_tick_gives_future_date() {
        let old = local(2026, 1, 1, 9);
        let now = local(2026, 1, 10, 12);
        let next = RepeatRule::Daily.next(old, true, now);
        assert!(next > now);
        assert_eq!(next.with_timezone(&Local).date_naive(), NaiveDate::from_ymd_opt(2026, 1, 11).unwrap());
    }

    #[test]
    fn swift_json_roundtrip() {
        let json = r#"[{"id":"11111111-AAAA-1111-1111-111111111111","title":"hello","isDone":false,"createdAt":"2026-01-05T09:00:00Z","deadline":"2026-12-01T09:00:00Z","deadlineHasTime":true}]"#;
        let items: Vec<TodoItem> = serde_json::from_str(json).unwrap();
        assert_eq!(items[0].title, "hello");
        assert!(items[0].deadline_has_time);
        assert_eq!(items[0].workspace_id, None);
        let out = serde_json::to_string(&items).unwrap();
        assert!(out.contains("2026-12-01T09:00:00Z"));
        assert!(out.contains("\"createdAt\":\"2026-01-05T09:00:00Z\""));
    }
}
