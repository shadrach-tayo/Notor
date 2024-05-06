use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::DateTime;
use futures::TryFutureExt;
use google_calendar::{AccessToken, calendar_list, Client, types::MinAccessRole};
use google_calendar::types::Event;
use crate::types::GoogleAuthToken;
use crate::utils::{EventGroups, with_local_timezone};

type PendingEventMap = HashMap<String, google_calendar::types::Event>;

#[derive(Clone)]
pub struct Calendars {
    pub accounts: Vec<CalenderAccount>,
    event_groups: EventGroups,
    pub pending_events: PendingEventMap,
}

impl Default for Calendars {
    fn default() -> Self {
        Calendars {
            accounts: vec![],
            event_groups: EventGroups::default(),
            pending_events: HashMap::new(),
        }
    }
}

impl Calendars {
    pub async fn new(tokens: Vec<GoogleAuthToken>) -> Self {
        let accounts = futures::future::join_all(tokens.iter().map(|token| async {
            CalenderAccount::new(token.to_owned()).await
        })).await;

        Calendars {
            accounts,
            event_groups: EventGroups::default(),
            pending_events: HashMap::new(),
        }
    }

    /// Add new calendar account to accounts list
    ///
    pub fn add_account(token: GoogleAuthToken) {
        todo!()
    }

    pub fn pending_events(&self) -> PendingEventMap {
        // todo!()
        let events: PendingEventMap = HashMap::new();
        events
    }

    pub async fn poll_events(&self) {
        let events = futures::future::join_all(self.accounts.iter().map(|account| async {
            account.get_calendar_events().await
        })).await;
        let events = events.iter().map(|e| e.to_owned()).flatten().collect::<Vec<Event>>();
        println!("Poll events {:?}", events)
    }
}

#[derive(Clone)]
pub struct CalenderAccount {
    token: Arc<Mutex<GoogleAuthToken>>,
    // email: Option<String>
    // userId: String
    // todo: omit in serialisation
    client: Client,

    // primary: bool [is primary account]
    // todo: omit in serialisation
    event_groups: EventGroups,
}

impl CalenderAccount {
    pub async fn new(token: GoogleAuthToken) -> Self {
        println!("Init Calendar account, {:?}", token);
        // todo: token refresh logic
        let mut client = Client::new(
            "",
            "",
            "",
            &token.access_token,
            token.refresh_token.clone().unwrap_or("".to_string()),
        );

        let client = client.set_auto_access_token_refresh(true);
        if token.expires_at.is_some() {
            let expires_at = token.expires_at.unwrap();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot retrieve system time");
            if expires_at as u64 > now.as_secs() {
                client.set_expires_in((expires_at as u64 - now.as_secs()) as i64).await;
            }
        }

        let expired = client.is_expired().await.unwrap_or(true);
        let mut token = token;
        if expired {
            let access_token = client.refresh_access_token().await;

            if let Ok(access_token) = access_token {
                println!("Access token refreshed");
                token.access_token = access_token.access_token;
                token.expires_in = access_token.expires_in;

                let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot retrieve system time");
                let expiry_date = chrono::DateTime::from_timestamp(
                    now.as_secs() as i64 + access_token.expires_in,
                    now.subsec_nanos(),
                )
                    .unwrap_or(DateTime::default());
                let expiry_date = with_local_timezone(expiry_date);
                println!("Token expiry date {:?}", &expiry_date);
                token.expires_at = Some(expiry_date.timestamp());

                println!("Token refreshed {:?}", &token);

                // logic to save tokens back to json file

                // let mut bytes: Vec<u8> = Vec::new();
                // serde_json::to_writer(&mut bytes, &raw_json_token).unwrap();
                // fs::write(&token_path, &bytes).map_err(|e| {
                //     println!("Error writing refresh token to file");
                //     e.to_string()
                // })?;
            } else {
                let err = access_token.err().unwrap();
                println!("Auth Error: {:?}", err);
                // let _ = open_auth_window(app).await;
            }
        };


        // todo: pull user accounts update (email, etc)
        let calendar_list = calendar_list::CalendarList::new(client.clone());
        let response = calendar_list
            .list(20, MinAccessRole::FreeBusyReader, "", true, true)
            .await;

        if response.is_ok() {
            dbg!(&response.unwrap().body);
        } else {
            println!("Error calendar error {:?}", response.err())
        }

        CalenderAccount {
            token: Arc::new(Mutex::new(token)),
            client: client.to_owned(),
            event_groups: EventGroups::default(),
        }
    }

    // use interior mutability pattern to set and update token
    pub async fn refresh_events(&self) {
        let calendar_list = calendar_list::CalendarList::new(self.client.clone());
        let response = calendar_list
            .list(20, MinAccessRole::FreeBusyReader, "", true, true)
            .await;
        dbg!(&response.unwrap().body);
        todo!()
    }

    pub async fn get_calendar_events(&self) -> Vec<Event> {
        let e: Vec<Event> = vec![];
        e
    }

    pub async fn is_token_expired(&self) -> Option<bool> {
        self.client.is_expired().await
    }

    pub async fn refresh_token(&self) -> Result<(), String> {
        let access_token = self.client.refresh_access_token().map_err(|err| err.to_string()).await?;
        // if let Ok(access_token) = access_token.unwrap() {
        // } else {
        //     Err("Error refreshing token".to_string())
        // }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot retrieve system time");
        let expiry_date = chrono::DateTime::from_timestamp(
            now.as_secs() as i64 + access_token.expires_in,
            now.subsec_nanos(),
        )
            .unwrap_or(DateTime::default());
        let expiry_date = with_local_timezone(expiry_date);

        let prev_token = self.token.lock().unwrap().clone();
        *self.token.lock().unwrap() = GoogleAuthToken {
            access_token: access_token.access_token,
            refresh_token: prev_token.refresh_token,
            expires_at: Some(expiry_date.timestamp()),
            expires_in: access_token.expires_in,
            token_type: prev_token.token_type,
            scope: prev_token.scope,
        };
        Ok(())
    }

    pub fn to_auth_token(&self) -> GoogleAuthToken {
        self.token.lock().unwrap().clone()
    }
}
