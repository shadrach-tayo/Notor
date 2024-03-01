use chrono::{DateTime, TimeZone, Timelike};
use chrono_humanize;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EventGroups {
    pub now: Vec<google_calendar::types::Event>,
    pub upcoming: Vec<google_calendar::types::Event>,
    pub tomorrow: Vec<google_calendar::types::Event>,
}

pub fn get_date_time(event: &google_calendar::types::Event) -> DateTime<chrono::Utc> {
    let t = event.start.clone().unwrap().date_time;

    if let Some(t) = t {
        t
    } else {
        let a = chrono::NaiveDateTime::parse_from_str(
            &event
                .start
                .clone()
                .unwrap()
                .date
                .unwrap()
                .clone()
                .to_string(),
            "%Y-%m-%dT%H:%M:%S",
        )
            .unwrap();
        let r = chrono::Local.from_local_datetime(&a).unwrap();
        println!("From naive date time {:?}", r);
        r.into()
    }
}

pub fn get_human_readable_end_time(event: google_calendar::types::Event) -> String {
    let dt = {
        let t = event.end.clone().unwrap().date_time;

        if let Some(t) = t {
            t
        } else {
            let a = chrono::NaiveDateTime::parse_from_str(
                &event.end.clone().unwrap().date.unwrap().to_string(),
                "%Y-%m-%dT%H:%M:%S",
            )
                .unwrap();
            let r = chrono::Local.from_local_datetime(&a).unwrap();
            println!("From naive date time {:?}", r);
            r.into()
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

pub fn get_human_start_time(event: google_calendar::types::Event) -> String {
    let dt = {
        let t = event.start.clone().unwrap().date_time;

        if let Some(t) = t {
            t
        } else {
            let a = chrono::NaiveDateTime::parse_from_str(
                &event
                    .start
                    .clone()
                    .unwrap()
                    .date
                    .unwrap()
                    .clone()
                    .to_string(),
                "%Y-%m-%dT%H:%M:%S",
            )
                .unwrap();
            let r = chrono::Local.from_local_datetime(&a).unwrap();
            println!("From naive date time {:?}", r);
            r.into()
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

pub fn get_human_readable_time(time: DateTime<chrono::Utc>) -> String {
    let (is_pm, hour) = time.hour12();
    let pm = if is_pm { "PM" } else { "AM" };
    let minute = time.minute();
    let minute = if minute < 10 {
        format!("0{}", minute)
    } else {
        minute.to_string()
    };
    format!("{}:{} {}", hour + 1, minute, pm)
}
