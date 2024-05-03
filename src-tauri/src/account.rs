use serde::{Deserialize, Serialize};
use crate::types::GoogleAuthToken;
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CalenderAccount {
    token: GoogleAuthToken
}

impl CalenderAccount {
    pub fn new(token: GoogleAuthToken) -> Self {
        CalenderAccount {
            token
        }
    }
}