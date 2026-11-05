use crate::resolve_local_datetime;
use chrono::{DateTime, NaiveTime, Timelike, Utc};
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
    let tz_str = iana_time_zone::get_timezone().unwrap_or_else(|_| chrono_tz::UTC.to_string());
    tz_str.parse().unwrap_or(Tz::UTC)
}

fn local_tz() -> Tz {
    system_timezone()
}

pub fn parse_event_datetime(
    event_datetime: google_calendar::types::EventDateTime,
) -> DateTime<Utc> {
    if let Some(datetime) = event_datetime.date_time {
        datetime
    } else {
        let (date, _) = chrono::NaiveDate::parse_and_remainder(
            &event_datetime.date.unwrap().to_string(),
            "%Y-%m-%d",
        )
        .unwrap();
        let date_with_time = date.and_time(NaiveTime::default());
        resolve_local_datetime(local_tz(), date_with_time).to_utc()
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
