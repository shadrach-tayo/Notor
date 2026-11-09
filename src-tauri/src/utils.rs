use chrono::{DateTime, Timelike, Utc};
use chrono_humanize;
use chrono_tz::Tz;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventGroups {
    pub now: Vec<google_calendar::types::Event>,
    pub upcoming: Vec<google_calendar::types::Event>,
    pub tomorrow: Vec<google_calendar::types::Event>,
}

pub fn system_timezone() -> Tz {
    crate::clock::system_timezone()
}

fn local_tz() -> Tz {
    system_timezone()
}

pub fn parse_event_datetime(
    event_datetime: google_calendar::types::EventDateTime,
) -> DateTime<Utc> {
    parse_event_datetime_in(event_datetime, local_tz())
}

pub fn parse_event_datetime_in(
    event_datetime: google_calendar::types::EventDateTime,
    tz: Tz,
) -> DateTime<Utc> {
    if let Some(datetime) = event_datetime.date_time {
        datetime
    } else {
        let date = event_datetime.date.expect("all-day event has a date");
        crate::event_grouping::all_day_start(tz, date).to_utc()
    }
}

pub fn with_local_timezone(date_time: DateTime<Utc>) -> DateTime<Tz> {
    date_time.with_timezone(&local_tz())
}

pub fn get_date_time(event: &google_calendar::types::Event) -> DateTime<Tz> {
    let datetime = parse_event_datetime(event.start.clone().unwrap());
    with_local_timezone(datetime)
}

pub fn time_to_relative_format(event_datetime: google_calendar::types::EventDateTime) -> String {
    let datetime = parse_event_datetime(event_datetime);
    let dt = with_local_timezone(datetime);
    chrono_humanize::HumanTime::from(dt).to_string()
}

pub fn get_human_readable_time(time: DateTime<Tz>) -> String {
    let hour24 = time.hour();

    let (_, hour) = time.hour12();
    let is_pm = hour24 >= 12;
    // println!("Hour {} {}", hour24, hour);
    let pm = if is_pm { "PM" } else { "AM" };
    let minute = time.minute();
    let minute = if minute < 10 {
        format!("0{}", minute)
    } else {
        minute.to_string()
    };
    format!("{}:{} {}", hour, minute, pm)
}
