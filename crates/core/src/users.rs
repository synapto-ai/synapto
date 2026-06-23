use derive_more::{Deref, IntoIterator};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::config::Config;
use synapto_interface::types::SpeakerId;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct UserId(String);

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct User {
    user_id: UserId,
    speaker_id: Option<SpeakerId>,
    pub full_name: String,
}

#[derive(Deref, IntoIterator, Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct Users(Vec<User>);

static USERS: Mutex<Users> = Mutex::new(Users::new());

impl Users {
    const fn new() -> Self {
        Self(Vec::new())
    }

    fn load(config: Config) {
        *USERS
            .lock()
            .unwrap_or_else(|e| panic!("USERS lock poisoned: {:?}", e)) =
            match std::fs::read_to_string(config.data_dir.join("users.json")) {
                Ok(json) => serde_json::from_str(&json)
                    .unwrap_or_else(|e| panic!("Failed to deserialize users: {}", e)),
                Err(_) => Users::new(),
            };
    }

    pub fn get_by_speaker_id(speaker_id: &SpeakerId) -> Option<User> {
        USERS
            .lock()
            .unwrap_or_else(|e| panic!("USERS lock poisoned: {:?}", e))
            .iter()
            .find(|u| u.speaker_id.as_ref() == Some(speaker_id))
            .cloned()
    }
}
