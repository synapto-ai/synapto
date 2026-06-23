use serde::{Deserialize, Serialize};

use synapto_interface::types::SpeakerId;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct UserId(String);

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct User {
    user_id: UserId,
    speaker_id: Option<SpeakerId>,
    pub full_name: String,
}

pub struct Users();

impl Users {
    pub fn get_by_speaker_id(speaker_id: &SpeakerId) -> Option<User> {
        Some(User {
            speaker_id: Some(speaker_id.clone()),
            user_id: UserId(speaker_id.0.clone()),
            full_name: speaker_id.0.clone(),
        })
    }
}
