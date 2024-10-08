use crate::types::{AccountPreference, AppCredentials, GoogleAuthToken, Preferences};
use crate::utils::{parse_event_datetime, with_local_timezone, EventGroups};
use chrono::{DateTime, Timelike};
use futures::TryFutureExt;
use google_calendar::events::Events;
use google_calendar::types::Event;
use google_calendar::{types::MinAccessRole, Client};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Calendars {
    accounts: tokio::sync::Mutex<Vec<CalenderAccount>>,
    config: AppCredentials,
    pub event_groups: Mutex<EventGroups>,
    events: Mutex<Vec<Event>>,
}

impl Default for Calendars {
    fn default() -> Self {
        Calendars {
            config: AppCredentials::default(),
            accounts: tokio::sync::Mutex::new(vec![]),
            event_groups: Mutex::new(EventGroups::default()),
            events: Mutex::new(Vec::new()),
        }
    }
}

impl Calendars {
    pub async fn new(
        tokens: Vec<GoogleAuthToken>,
        config: AppCredentials,
        preferences: &Preferences,
    ) -> Self {
        let accounts = futures::future::join_all(tokens.iter().map(|token| async {
            let email = token.user.clone().unwrap().email;
            let account_preferences = preferences.get_account_preference(&email);
            CalenderAccount::new(token.to_owned(), config.clone(), account_preferences).await
        }))
        .await;

        Calendars {
            config,
            accounts: tokio::sync::Mutex::new(accounts),
            event_groups: Mutex::new(EventGroups::default()),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Add new calendar account to accounts list
    pub async fn add_account(
        &self,
        token: GoogleAuthToken,
        preferences: &Preferences,
    ) -> Result<(), String> {
        println!("add_account::Locked---------+++++++");
        if token.user.is_some() {
            println!("Add new Account");
            let mut calendar_accounts = self.accounts.lock().await;

            println!("Lock acquired");
            let accounts = calendar_accounts
                .iter()
                .filter_map(|account| {
                    if account.is_account(&token.user.clone().unwrap().email) {
                        None
                    } else {
                        Some(account)
                    }
                })
                .collect::<Vec<&CalenderAccount>>();
            let mut tokens = accounts
                .iter()
                .map(|acct| acct.to_auth_token())
                .collect::<Vec<GoogleAuthToken>>();
            tokens.insert(tokens.len(), token);

            let accounts = futures::future::join_all(tokens.iter().map(|token| async {
                let email = token.user.clone().unwrap().email;
                let account_preferences = preferences.get_account_preference(&email);
                CalenderAccount::new(token.to_owned(), self.config.clone(), account_preferences)
                    .await
            }))
            .await;

            *calendar_accounts = accounts;
            drop(calendar_accounts);
        }
        Ok(())
    }

    pub async fn remove_account(
        &self,
        email: String,
        preferences: &Preferences,
    ) -> Result<(), String> {
        let mut calendar_accounts = self.accounts.lock().await;

        let accounts = calendar_accounts
            .iter()
            .filter(|account| !account.is_account(&email))
            .collect::<Vec<&CalenderAccount>>();

        if calendar_accounts.len() == 1 {
            return Ok(());
        }

        let tokens = accounts
            .iter()
            .map(|acct| acct.to_auth_token())
            .collect::<Vec<GoogleAuthToken>>();

        let accounts = futures::future::join_all(tokens.iter().map(|token| async {
            let email = token.user.clone().unwrap().email;
            let account_preferences = preferences.get_account_preference(&email);
            CalenderAccount::new(token.to_owned(), self.config.clone(), account_preferences).await
        }))
        .await;

        *calendar_accounts = accounts;
        drop(calendar_accounts);
        Ok(())
    }

    pub async fn disable_account(&self, email: String) -> Result<(), String> {
        let calendar_accounts = self.accounts.lock().await;

        let account = calendar_accounts
            .iter()
            .find(|account| account.is_account(&email));

        if let Some(account) = account {
            account.disable();
            println!("Disabled {}: {}", &email, account.is_diabled());
        }

        drop(calendar_accounts);
        self.poll_events().await;

        Ok(())
    }

    pub async fn enable_account(&self, email: String) -> Result<(), String> {
        let calendar_accounts = self.accounts.lock().await;

        let account = calendar_accounts
            .iter()
            .find(|account| account.is_account(&email));

        if let Some(account) = account {
            account.enable();
            println!("Enable {}: {}", &email, account.is_diabled());
        }

        drop(calendar_accounts);
        self.poll_events().await;

        Ok(())
    }

    pub async fn get_tokens(&self) -> Result<Vec<GoogleAuthToken>, String> {
        let tokens = self
            .accounts
            .lock()
            .await
            .iter()
            .map(|account| account.to_auth_token())
            .collect::<Vec<GoogleAuthToken>>();
        Ok(tokens)
    }

    pub fn active_events(&self) -> Vec<Event> {
        self.event_groups.lock().unwrap().now.clone()
    }

    pub fn upcoming_events(&self) -> Vec<Event> {
        self.event_groups.lock().unwrap().upcoming.clone()
    }

    pub fn tomorrow_events(&self) -> Vec<Event> {
        self.event_groups.lock().unwrap().tomorrow.clone()
    }

    pub fn group_events(&self) {
        let events = self.events.lock().unwrap();

        let mut groups = EventGroups::default();

        if events.is_empty() {
            *self.event_groups.lock().unwrap() = groups;
            return;
        }

        let now = chrono::offset::Local::now();
        let tomorrow = chrono::offset::Local::now()
            .checked_add_days(chrono::naive::Days::new(1))
            .unwrap()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap();
        let tomorrow_end = chrono::offset::Local::now()
            .checked_add_days(chrono::naive::Days::new(1))
            .unwrap()
            .with_hour(23)
            .unwrap()
            .with_minute(59)
            .unwrap()
            .with_second(0)
            .unwrap();

        // println!("Now {:?} - Tomorrow {:?} - Tomorrow End {:?}", &now, &tomorrow, &tomorrow_end);

        for event in events.iter() {
            let start = with_local_timezone(parse_event_datetime(event.start.clone().unwrap()));
            let end = with_local_timezone(parse_event_datetime(event.end.clone().unwrap()));

            if now > start && now < end {
                groups.now.push(event.to_owned());
            } else if now < start && start < tomorrow {
                groups.upcoming.push(event.to_owned());
            } else if start > tomorrow && start < tomorrow_end {
                groups.tomorrow.push(event.to_owned())
            }
            // println!("Event: {}, start: {:?}", &event.summary, start);
        }
        println!("Polled events {:?}", events.len());
        groups
            .now
            .sort_by_key(|event| parse_event_datetime(event.end.clone().unwrap()));
        groups
            .upcoming
            .sort_by_key(|event| parse_event_datetime(event.start.clone().unwrap()));
        groups
            .tomorrow
            .sort_by_key(|event| parse_event_datetime(event.start.clone().unwrap()));

        // println!("Now Groups {:?}", groups.now.iter().map(|g| &g.summary).collect::<Vec<&String>>());
        // println!("Upcoming Groups {:?}", groups.upcoming.iter().map(|g| &g.summary).collect::<Vec<&String>>());
        // println!("Tomorrow Groups {:?}", groups.tomorrow.iter().map(|g| &g.summary).collect::<Vec<&String>>());
        *self.event_groups.lock().unwrap() = groups;
    }

    pub async fn poll_events(&self) {
        let accounts = self.accounts.lock().await;
        let events =
            futures::future::join_all(accounts.iter().filter(|account| !account.is_diabled()).map(
                |account| async {
                    println!("Account to poll: {}", account.is_diabled());
                    account.get_calendar_events().await
                },
            ))
            .await;

        let events = events
            .iter()
            .map(|e| e.to_owned())
            .flatten()
            .collect::<Vec<Event>>();
        println!("Poll events {:?}", events.len());

        *self.events.lock().unwrap() = events;

        self.group_events();
    }

    pub async fn set_preferences(&self, email: String, preferences: &Preferences) {
        let calendar_accounts = self.accounts.lock().await;

        let account = calendar_accounts
            .iter()
            .find(|account| account.is_account(&email));

        if let Some(account) = account {
            let account_preference = preferences.get_account_preference(&email);
            println!(
                "Account Preferences Set {}: {:?}",
                &email, &account_preference
            );
            account.set_preferences(account_preference).await;
        }

        drop(calendar_accounts);
        self.poll_events().await;
    }
}

pub struct CalenderAccount {
    token: Arc<Mutex<GoogleAuthToken>>,
    calendar_list: Vec<google_calendar::types::CalendarListEntry>,
    events: Events,
    client: Client,
    #[allow(dead_code)]
    event_groups: EventGroups,
    // disabled: Option<bool>,
    preferences: tokio::sync::Mutex<AccountPreference>,
}

impl CalenderAccount {
    pub async fn new(
        token: GoogleAuthToken,
        client_config: AppCredentials,
        preferences: AccountPreference,
    ) -> Self {
        let account_email = &token.clone().user.unwrap().email;
        println!("Init Calendar account, {}", &account_email);
        let mut client = Client::new(
            client_config.google_client_id,
            client_config.google_client_secret,
            client_config.google_redirect_url,
            &token.access_token,
            token.refresh_token.clone().unwrap_or("".to_string()),
        );

        let client = client.set_auto_access_token_refresh(true);
        if token.expires_at.is_some() {
            let expires_at = token.expires_at.unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("cannot retrieve system time");
            if expires_at as u64 > now.as_secs() {
                client
                    .set_expires_in((expires_at as u64 - now.as_secs()) as i64)
                    .await;
            }
        }

        let expired = client.is_expired().await.unwrap_or(true);
        let mut token = token;
        if expired {
            let access_token = client.refresh_access_token().await;

            if let Ok(access_token) = access_token {
                println!("Access token refreshed: {:?}", access_token);
                if !access_token.access_token.is_empty() {
                    token.access_token = access_token.access_token;
                }

                if access_token.expires_in != 0 {
                    token.expires_in = access_token.expires_in;
                }

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("cannot retrieve system time");
                let expiry_date = chrono::DateTime::from_timestamp(
                    now.as_secs() as i64 + access_token.expires_in,
                    now.subsec_nanos(),
                )
                .unwrap_or(DateTime::default());
                let expiry_date = with_local_timezone(expiry_date);
                println!("New Token expiry date - {:?}", &expiry_date);
                token.expires_at = Some(expiry_date.timestamp());

                // println!("Token refreshed {:?}", &token);

                // logic to save tokens back to json file

                // let mut bytes: Vec<u8> = Vec::new();
                // serde_json::to_writer(&mut bytes, &raw_json_token).unwrap();
                // fs::write(&token_path, &bytes).map_err(|e| {
                //     println!("Error writing refresh token to file");
                //     e.to_string()
                // })?;
            } else {
                let err = access_token.err().unwrap();
                println!("Auth Error: {} : {:?}", &account_email, err);
                // let _ = open_auth_window(app).await;
            }
        };

        // todo: pull user accounts update (email, etc)
        let calendar_list = client.calendar_list();
        let response = calendar_list
            .list(20, MinAccessRole::FreeBusyReader, "", false, false)
            .await;

        let calendar_list = if response.is_ok() {
            let list = response.unwrap().body;
            println!("CalendarListEntry {:?}", list.len());
            list
        } else {
            println!(
                "Error listing calendar {account_email} {:?}",
                response.err()
            );
            vec![]
        };

        let events = Events::new(client.clone());
        CalenderAccount {
            events,
            calendar_list,
            preferences: tokio::sync::Mutex::new(preferences),
            token: Arc::new(Mutex::new(token)),
            client: client.to_owned(),
            event_groups: EventGroups::default(),
        }
    }

    pub fn is_account(&self, email: &str) -> bool {
        let user = self.token.lock().unwrap().clone().user;

        if user.is_some() {
            return &user.unwrap().email.clone() == email;
        }

        false
    }

    pub async fn get_calendar_events(&self) -> Vec<Event> {
        // println!("Is token expired for {}", self.token.lock().unwrap().clone().user.unwrap().email);
        let account_email = self.token.lock().unwrap().clone().user.unwrap().email;
        if self.is_token_expired().await {
            println!("Token expired for account: {}", &account_email);
            if let Err(err) = self.client.refresh_access_token().await {
                println!("Refresh token Error: {} {:?}", &account_email, err);
                return vec![];
            }
        }

        let time_min = chrono::offset::Local::now()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap();

        let time_max = chrono::offset::Local::now()
            .checked_add_days(chrono::naive::Days::new(3))
            .unwrap()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap();

        // println!(
        //     "time min {:?} time max {:?}",
        //     time_min.to_rfc3339(),
        //     time_max.to_rfc3339()
        // );
        // let account_email = self.token.lock().unwrap().clone().user.unwrap().email;
        let preferences = self.preferences.lock().await;
        let events = futures::future::join_all(
            self.calendar_list
                .iter()
                .filter(|calendar| !preferences.hidden_calendars.contains(&calendar.id))
                .map(|entry| async {
                    let response = self
                        .events
                        .list(
                            &entry.id,
                            "",
                            0,
                            0,
                            google_calendar::types::OrderBy::Noop,
                            "",
                            &[],
                            "",
                            &[],
                            false,
                            false,
                            true,
                            &time_max.to_rfc3339(),
                            &time_min.to_rfc3339(),
                            "",
                            "",
                        )
                        .await;
                    // let response = response.unwrap();
                    if let Ok(response) = response {
                        if response.status.is_success() {
                            let body = response.body;
                            // println!("Fetch events success: {}: {} {}", &entry.id, response.status.to_string(), body.len());
                            body.iter()
                                .filter_map(|event| {
                                    let is_creator = {
                                        let creator = &event.creator;
                                        if let Some(creator) = creator {
                                            creator.email == account_email
                                        } else {
                                            false
                                        }
                                    };

                                    if is_creator {
                                        return Some(event.to_owned());
                                    }

                                    let is_user_attendee =
                                        event.attendees.iter().find(|attendee| {
                                            attendee.email
                                                == self
                                                    .token
                                                    .lock()
                                                    .unwrap()
                                                    .clone()
                                                    .user
                                                    .unwrap()
                                                    .email
                                        });

                                    if is_user_attendee.is_some() {
                                        Some(event.to_owned())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<Event>>()
                        } else {
                            println!("Fetch events error: {}", response.status.to_string());
                            vec![]
                        }
                    } else {
                        println!("Fetch event Error: {} - {:?}", &entry.id, response.err());
                        vec![]
                    }
                }),
        )
        .await;

        events
            .iter()
            .flatten()
            .map(|e| e.to_owned())
            .collect::<Vec<Event>>()
    }

    pub async fn is_token_expired(&self) -> bool {
        match self.client.is_expired().await {
            Some(is_expired) => is_expired,
            None => true,
        }
    }

    pub async fn refresh_token(&self) -> Result<Option<bool>, String> {
        if let None = self.token.lock().unwrap().refresh_token {
            return Ok(None);
        }

        if !self.is_token_expired().await {
            return Ok(None);
        }

        let access_token = self
            .client
            .refresh_access_token()
            .map_err(|err| err.to_string())
            .await?;

        if access_token.access_token.is_empty() {
            return Ok(Some(false));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("cannot retrieve system time");
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
            user: prev_token.user,
            disabled: prev_token.disabled,
        };

        Ok(Some(true))
    }

    pub fn to_auth_token(&self) -> GoogleAuthToken {
        self.token.lock().unwrap().clone()
    }

    // pub fn get_state(&self) -> StateToken {
    //     StateToken {
    //         token: self.token.lock().unwrap().clone(),
    //         disabled: self.disabled,
    //     }
    // }

    pub fn disable(&self) {
        self.token.lock().unwrap().disabled = Some(true);
    }

    pub fn enable(&self) {
        self.token.lock().unwrap().disabled = Some(false);
    }

    pub fn is_diabled(&self) -> bool {
        self.token.lock().unwrap().disabled.unwrap_or(false)
    }

    pub fn is_enabled(&self) -> bool {
        !self.is_diabled()
    }

    pub async fn set_preferences(&self, account_preference: AccountPreference) {
        *self.preferences.lock().await = account_preference;
    }
}
