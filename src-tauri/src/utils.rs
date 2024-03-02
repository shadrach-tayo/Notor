use chrono::{DateTime, TimeZone, Timelike};
use chrono_humanize;
use chrono_tz::Tz;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EventGroups {
    pub now: Vec<google_calendar::types::Event>,
    pub upcoming: Vec<google_calendar::types::Event>,
    pub tomorrow: Vec<google_calendar::types::Event>,
}

pub fn get_date_time(event: &google_calendar::types::Event) -> DateTime<Tz> {
    let t = event.start.clone().unwrap().date_time;

    let parsed_time_zone = event.start.clone().unwrap().time_zone.replace("%2F", "/");
    let tz: Tz = parsed_time_zone.parse().unwrap_or_else(|_| {
        Tz::UTC
    });

    if let Some(t) = t {
        t.with_timezone(&tz)
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
        println!("From naive date time {:?}", r.with_timezone(&tz));
        r.with_timezone(&tz)
    }
}

pub fn get_human_readable_end_time(event: google_calendar::types::Event) -> String {
    let dt = {
        let t = event.end.clone().unwrap().date_time;

        let parsed_time_zone = event.start.clone().unwrap().time_zone.replace("%2F", "/");
        let tz: Tz = parsed_time_zone.parse().unwrap_or_else(|_| {
            Tz::UTC
        });

        if let Some(t) = t {
            t.with_timezone(&tz)
        } else {
            let a = chrono::NaiveDateTime::parse_from_str(
                &event.end.clone().unwrap().date.unwrap().to_string(),
                "%Y-%m-%dT%H:%M:%S",
            )
                .unwrap();
            let r = chrono::Local.from_local_datetime(&a).unwrap();
            println!("From naive date time {:?}", r);
            r.with_timezone(&tz)
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

pub fn get_human_start_time(event: google_calendar::types::Event) -> String {
    // println!("{}: time:{:?}, timezone: {}", event.summary, event.start.clone().unwrap().date_time, event.start.clone().unwrap().time_zone);
    let parsed_time_zone = event.start.clone().unwrap().time_zone.replace("%2F", "/");
    let tz: Tz = parsed_time_zone.parse().unwrap_or_else(|_| {
        Tz::UTC
    });

    let dt = {
        let t = event.start.clone().unwrap().date_time;

        if let Some(t) = t {
            let converted_time = t.with_timezone(&tz);
            converted_time
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
            let converted_time = r.with_timezone(&tz);
            println!("From naive date time {:?}", converted_time);
            // r.into()
            converted_time
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

pub fn get_human_readable_time(time: DateTime<Tz>) -> String {
    let hour24 = time.hour();

    let (_, hour) = time.hour12();
    let is_pm = hour24 >= 12;

    let pm = if is_pm { "PM" } else { "AM" };
    let minute = time.minute();
    let minute = if minute < 10 {
        format!("0{}", minute)
    } else {
        minute.to_string()
    };
    format!("{}:{} {}", hour, minute, pm)
}
