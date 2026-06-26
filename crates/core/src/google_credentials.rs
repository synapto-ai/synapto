use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct GoogleServiceAccountCredentials(serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(value: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&value.0).unwrap_or_else(|e| {
            panic!("Failed to serialize GoogleServiceAccountCredentials: {}", e)
        })
    }
}
