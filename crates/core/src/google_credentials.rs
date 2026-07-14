use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(creds: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&creds)
            .unwrap_or_else(|_| panic!("Failed to parse google service account credentials"))
    }
}
