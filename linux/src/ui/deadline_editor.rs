use chrono::{Datelike, Duration, Local, NaiveDate, Timelike, Utc};
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Point, Pixels};
use uuid::Uuid;

use crate::model::{local_at, local_start_of_day, RepeatRule};
use crate::ui::glass::Palette;
use crate::ui::header::{menu_box, menu_divider};
use crate::ui::panel::icon;
use crate::ui::root::*;

#[derive(Clone, Copy, PartialEq)]
pub enum DeadlineTarget {
    New,
    Task(Uuid),
}

#[derive(Clone)]
pub struct DeadlineState {
    pub target: DeadlineTarget,
    /// First day of the month being shown.
    pub month: NaiveDate,
    pub date: NaiveDate,
    pub include_time: bool,
    pub hour: u32,
    pub minute: u32,
    pub repeat: Option<RepeatRule>,
    pub repeat_open: bool,
    pub had_deadline: bool,
}

impl DeadlineState {
    pub fn fresh(target: DeadlineTarget) -> Self {
        let today = Local::now().date_naive();
        Self {
            target,
            month: today.with_day(1).unwrap(),
            date: today,
            include_time: false,
            hour: 17,
            minute: 0,
            repeat: None,
            repeat_open: false,
            had_deadline: false,
        }
    }

    pub fn from_existing(
        d: chrono::DateTime<Utc>,
        has_time: bool,
        repeat: Option<RepeatRule>,
        had_deadline: bool,
        target: DeadlineTarget,
    ) -> Self {
        let l = d.with_timezone(&Local);
        let date = l.date_naive();
        Self {
            target,
            month: date.with_day(1).unwrap(),
            date,
            include_time: has_time,
            hour: if has_time { l.hour() } else { 17 },
            minute: if has_time { l.minute() } else { 0 },
            repeat,
            repeat_open: false,
            had_deadline,
        }
    }

    fn value(&self) -> (chrono::DateTime<Utc>, bool, Option<RepeatRule>) {
        let d = if self.include_time {
            local_at(self.date, self.hour, self.minute)
        } else {
            local_start_of_day(self.date)
        };
        (d, self.include_time, self.repeat)
    }

    fn shift_month(&mut self, delta: i32) {
        let total = self.month.year() * 12 + self.month.month0() as i32 + delta;
        self.month = NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).unwrap();
    }
}

fn time_label(h: u32, m: u32) -> String {
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{}:{:02} {}", h12, m, if h >= 12 { "PM" } else { "AM" })
}

impl Root {
    fn with_state(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut DeadlineState)) {
        if let Some(Overlay::Deadline(s)) = &mut self.overlay {
            f(s);
        }
        cx.notify();
    }

    fn commit_deadline(&mut self, state: DeadlineState, cx: &mut Context<Self>) {
        let (date, has_time, repeat) = state.value();
        self.overlay = None;
        match state.target {
            DeadlineTarget::New => {
                self.new_deadline = Some(PendingDeadline { date, has_time, repeat });
                self.needs_sync = true;
                cx.notify();
            }
            DeadlineTarget::Task(id) => self.edit_store(cx, |s| s.set_deadline(id, Some(date), has_time, repeat)),
        }
    }

    fn remove_deadline(&mut self, state: DeadlineState, cx: &mut Context<Self>) {
        self.overlay = None;
        match state.target {
            DeadlineTarget::New => {
                self.new_deadline = None;
                cx.notify();
            }
            DeadlineTarget::Task(id) => self.edit_store(cx, |s| s.set_deadline(id, None, false, None)),
        }
    }

    pub fn deadline_popover(
        &mut self,
        state: DeadlineState,
        pos: Point<Pixels>,
        pal: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.overlay = Some(Overlay::Deadline(state.clone()));
        let today = Local::now().date_naive();

        let quick = |label: &'static str, days: i64, cx: &mut Context<Self>| {
            div()
                .id(label)
                .debug_selector(move || label.to_string())
                .flex_1()
                .flex()
                .justify_center()
                .py(px(3.))
                .rounded(px(5.))
                .bg(pal.hover)
                .text_size(px(11.5))
                .cursor_pointer()
                .hover(move |s| s.bg(pal.selected))
                .on_click(cx.listener(move |this, _e, _w, cx| {
                    if let Some(Overlay::Deadline(s)) = &this.overlay {
                        let mut s = s.clone();
                        s.date = Local::now().date_naive() + Duration::days(days);
                        this.commit_deadline(s, cx);
                    }
                }))
                .child(label)
        };

        // Calendar grid: 6 weeks starting on the Sunday on/before the 1st.
        let first = state.month;
        let lead = first.weekday().num_days_from_sunday() as i64;
        let grid_start = first - Duration::days(lead);
        let mut weeks: Vec<AnyElement> = vec![];
        for w in 0..6 {
            let mut row = div().flex().justify_between();
            for d in 0..7 {
                let day = grid_start + Duration::days(w * 7 + d);
                let in_month = day.month() == first.month();
                let selected = day == state.date;
                let is_today = day == today;
                let mut cell = div()
                    .id(format!("day-{}", day))
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .text_color(if selected {
                        gpui::white()
                    } else if in_month {
                        pal.fg
                    } else {
                        pal.tertiary
                    })
                    .on_click(cx.listener(move |this, _e, _w, cx| {
                        this.with_state(cx, |s| {
                            s.date = day;
                            s.month = day.with_day(1).unwrap();
                        })
                    }))
                    .child(day.day().to_string());
                if selected {
                    cell = cell.bg(pal.accent);
                } else {
                    cell = cell.hover(move |s| s.bg(pal.hover));
                    if is_today {
                        cell = cell.border_1().border_color(pal.accent);
                    }
                }
                row = row.child(cell);
            }
            weeks.push(row.into_any_element());
        }

        let small_btn = |id: &'static str, label: &'static str, cx: &mut Context<Self>, f: fn(&mut DeadlineState)| {
            div()
                .id(id)
                .px(px(5.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(pal.hover)
                .text_size(px(11.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _e, _w, cx| this.with_state(cx, f)))
                .child(label)
        };

        let time_row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id("time-check")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _e, _w, cx| this.with_state(cx, |s| s.include_time = !s.include_time)))
                    .child(
                        div()
                            .size(px(14.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(pal.secondary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(state.include_time, |d| d.bg(pal.accent).border_color(pal.accent).child(icon("check", 10.0, gpui::white()))),
                    )
                    .child(div().text_size(px(12.)).child("Time")),
            )
            .child(div().flex_1())
            .when(state.include_time, |d| {
                d.child(small_btn("h-", "−1h", cx, |s| s.hour = (s.hour + 23) % 24))
                    .child(small_btn("m-", "−5m", cx, |s| s.minute = (s.minute + 55) % 60))
                    .child(div().text_size(px(11.5)).w(px(54.)).flex().justify_center().child(time_label(state.hour, state.minute)))
                    .child(small_btn("m+", "+5m", cx, |s| s.minute = (s.minute + 5) % 60))
                    .child(small_btn("h+", "+1h", cx, |s| s.hour = (s.hour + 1) % 24))
            });

        let repeat_title = state.repeat.map_or("Never", |r| r.title());
        let mut repeat_box = div().flex().flex_col().gap(px(2.)).child(
            div()
                .id("repeat-toggle")
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .on_click(cx.listener(|this, _e, _w, cx| this.with_state(cx, |s| s.repeat_open = !s.repeat_open)))
                .child(div().text_size(px(12.)).child("Repeat"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(5.))
                        .bg(pal.hover)
                        .text_size(px(11.5))
                        .child(repeat_title)
                        .child(icon("chevron-down", 9.0, pal.secondary)),
                ),
        );
        if state.repeat_open {
            let mut options: Vec<(String, Option<RepeatRule>)> = vec![("Never".into(), None)];
            options.extend(RepeatRule::ALL.iter().map(|r| (r.title().to_string(), Some(*r))));
            for (i, (label, rule)) in options.into_iter().enumerate() {
                let selected = state.repeat == rule;
                repeat_box = repeat_box.child(
                    div()
                        .id(format!("rep-{i}"))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(6.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(pal.selected))
                        .text_size(px(11.5))
                        .on_click(cx.listener(move |this, _e, _w, cx| {
                            this.with_state(cx, |s| {
                                s.repeat = rule;
                                s.repeat_open = false;
                            })
                        }))
                        .child(div().w(px(12.)).when(selected, |d| d.child(icon("check", 10.0, pal.fg))))
                        .child(label),
                );
            }
        }

        let commit_state = state.clone();
        let remove_state = state.clone();
        let actions = div()
            .flex()
            .items_center()
            .child(div().when(state.had_deadline, |d| {
                d.child(
                    div()
                        .id("dl-remove")
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(pal.hover)
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _e, _w, cx| this.remove_deadline(remove_state.clone(), cx)))
                        .child("Remove"),
                )
            }))
            .child(div().flex_1())
            .child(
                div()
                    .id("dl-set")
                    .debug_selector(|| "dl-set".into())
                    .px(px(14.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(pal.accent)
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _e, _w, cx| {
                        // Use the latest edited state, not the one captured at render time.
                        let latest = match &this.overlay {
                            Some(Overlay::Deadline(s)) => s.clone(),
                            _ => commit_state.clone(),
                        };
                        this.commit_deadline(latest, cx)
                    }))
                    .child("Set"),
            );

        let month_name = state.month.format("%B %Y").to_string();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(self.icon_button("mo-prev", "chevron-left", 22.0, 12.0, pal, cx, |this, _w, cx| {
                this.with_state(cx, |s| s.shift_month(-1))
            }))
            .child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(12.5)).child(month_name))
            .child(self.icon_button("mo-next", "chevron-right", 22.0, 12.0, pal, cx, |this, _w, cx| {
                this.with_state(cx, |s| s.shift_month(1))
            }));
        let weekdays = div().flex().justify_between().children(["S", "M", "T", "W", "T", "F", "S"].map(|d| {
            div().w(px(28.)).flex().justify_center().text_size(px(10.)).text_color(pal.tertiary).child(d)
        }));

        div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .w(px(236.))
            .child(
                menu_box(pal)
                    .p(px(12.))
                    .gap(px(8.))
                    .child(div().flex().gap(px(6.)).child(quick("Today", 0, cx)).child(quick("Tomorrow", 1, cx)).child(quick("Next week", 7, cx)))
                    .child(header)
                    .child(weekdays)
                    .child(div().flex().flex_col().gap(px(1.)).children(weeks))
                    .child(menu_divider(pal).my(px(0.)))
                    .child(time_row)
                    .child(repeat_box)
                    .child(actions),
            )
            .into_any_element()
    }
}
