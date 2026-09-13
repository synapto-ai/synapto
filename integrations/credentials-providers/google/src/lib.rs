use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use synapto_interface::credentials::{
    CredentialsBuilder, PluggableCredentialsProvider, ProvideBearerToken,
};
use synapto_interface::secrets::Secret;
use tokio::sync::RwLock;

/// Target descriptor for Google Cloud authentication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GoogleCloudTarget {
    pub scopes: Vec<String>,
}

impl synapto_interface::credentials::CredentialTarget for GoogleCloudTarget {}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub private_key_id: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub client_email: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub auth_uri: Option<String>,
    #[serde(default)]
    pub token_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoogleCredentialsConfig {
    #[serde(default)]
    pub service_account_key: Option<Secret<String>>,
    #[serde(default)]
    pub service_account: Option<GoogleServiceAccountCredentials>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub client_email: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub token_uri: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Clone)]
struct CachedToken {
    token: Secret<String>,
    expires_at: std::time::Instant,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    exp: i64,
    iat: i64,
}

pub struct GoogleCredentials {
    config: GoogleCredentialsConfig,
    token_cache: RwLock<HashMap<BTreeSet<String>, CachedToken>>,
    http_client: Client,
}

impl GoogleCredentials {
    pub fn new(config: GoogleCredentialsConfig) -> Self {
        Self {
            config,
            token_cache: RwLock::new(HashMap::new()),
            http_client: Client::new(),
        }
    }

    fn extract_service_account_credentials(
        &self,
    ) -> Result<Option<GoogleServiceAccountCredentials>, String> {
        if let Some(ref direct) = self.config.service_account
            && !direct.private_key.is_empty()
        {
            return Ok(Some(direct.clone()));
        }

        if let Some(ref key_secret) = self.config.service_account_key {
            let key_str = key_secret.expose_secret().trim();
            if !key_str.is_empty() {
                if let Ok(creds) = serde_json::from_str::<GoogleServiceAccountCredentials>(key_str)
                    && !creds.private_key.is_empty()
                {
                    return Ok(Some(creds));
                }

                // If parsing whole JSON failed, treat key_str as PEM private key directly
                if key_str.contains("PRIVATE KEY") {
                    let creds = GoogleServiceAccountCredentials {
                        project_id: self.config.project_id.clone().unwrap_or_default(),
                        private_key_id: String::new(),
                        private_key: key_str.to_string(),
                        client_email: self.config.client_email.clone().unwrap_or_default(),
                        client_id: self.config.client_id.clone().unwrap_or_default(),
                        auth_uri: None,
                        token_uri: self.config.token_uri.clone(),
                    };
                    return Ok(Some(creds));
                }
            }
        }

        Ok(None)
    }

    async fn fetch_token_via_jwt(
        &self,
        creds: &GoogleServiceAccountCredentials,
        target: &GoogleCloudTarget,
    ) -> Result<(Secret<String>, std::time::Duration), String> {
        let now = Utc::now().timestamp();
        let default_token_uri = if let Ok(base) = std::env::var("GOOGLE_API_BASE_URL") {
            format!("{}/token", base.trim_end_matches('/'))
        } else {
            "https://oauth2.googleapis.com/token".to_string()
        };

        let token_uri = creds
            .token_uri
            .as_deref()
            .unwrap_or(&default_token_uri);

        let scopes_joined = target.scopes.join(" ");
        let claims = JwtClaims {
            iss: &creds.client_email,
            scope: &scopes_joined,
            aud: token_uri,
            exp: now + 3600,
            iat: now,
        };

        let key = jsonwebtoken::EncodingKey::from_rsa_pem(creds.private_key.as_bytes())
            .map_err(|e| format!("Invalid Google Service Account RSA private key: {e}"))?;
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        let assertion = jsonwebtoken::encode(&header, &claims, &key)
            .map_err(|e| format!("Failed to create Google OAuth JWT assertion: {e}"))?;

        let response = self
            .http_client
            .post(token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(|e| format!("Google OAuth token request failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Google OAuth token request returned error: {e}"))?;

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Google OAuth token response: {e}"))?;

        Ok((
            Secret::new(token_response.access_token),
            std::time::Duration::from_secs(token_response.expires_in),
        ))
    }

    async fn fetch_token_via_ambient_metadata(
        &self,
        target: &GoogleCloudTarget,
    ) -> Result<(Secret<String>, std::time::Duration), String> {
        let metadata_base = std::env::var("GOOGLE_METADATA_SERVER_URL")
            .unwrap_or_else(|_| "http://169.254.169.254".to_string());
        let mut url = reqwest::Url::parse(&format!(
            "{}/computeMetadata/v1/instance/service-accounts/default/token",
            metadata_base.trim_end_matches('/')
        ))
        .map_err(|e| e.to_string())?;

        if !target.scopes.is_empty() {
            url.query_pairs_mut()
                .append_pair("scopes", &target.scopes.join(","));
        }

        let response = self
            .http_client
            .get(url)
            .header("Metadata-Flavor", "Google")
            .timeout(std::time::Duration::from_millis(1500))
            .send()
            .await
            .map_err(|e| {
                format!(
                    "GCP Compute Engine Metadata Server unreachable ({e}). \
                     No static service account credentials configured and ambient VM identity unavailable."
                )
            })?;

        if !response.status().is_success() {
            return Err(format!(
                "GCP Compute Engine Metadata Server returned HTTP {}",
                response.status()
            ));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse metadata token response: {e}"))?;

        let access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| "Missing access_token in metadata response".to_string())?;
        let expires_in_secs = json["expires_in"].as_u64().unwrap_or(3600);

        Ok((
            Secret::new(access_token.to_string()),
            std::time::Duration::from_secs(expires_in_secs),
        ))
    }
}

#[async_trait]
impl ProvideBearerToken<GoogleCloudTarget> for GoogleCredentials {
    async fn resolve_bearer_token(
        &self,
        target: &GoogleCloudTarget,
    ) -> Result<Secret<String>, String> {
        let normalized_scopes: BTreeSet<String> = target.scopes.iter().cloned().collect();

        // 1. Check in-memory scope cache with 60-second expiration buffer
        {
            let cache = self.token_cache.read().await;
            if let Some(entry) = cache.get(&normalized_scopes)
                && entry.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(60)
            {
                return Ok(entry.token.clone());
            }
        }

        // 2. Fetch token via static credentials or ambient VM metadata
        let (token, ttl) = if let Some(creds) = self.extract_service_account_credentials()? {
            self.fetch_token_via_jwt(&creds, target).await?
        } else {
            self.fetch_token_via_ambient_metadata(target).await?
        };

        // 3. Save into cache
        let mut cache = self.token_cache.write().await;
        cache.insert(
            normalized_scopes,
            CachedToken {
                token: token.clone(),
                expires_at: std::time::Instant::now() + ttl,
            },
        );

        Ok(token)
    }
}

impl PluggableCredentialsProvider for GoogleCredentials {
    type Config = GoogleCredentialsConfig;

    fn register_provider(
        config: Self::Config,
        builder: &mut CredentialsBuilder,
    ) -> Result<(), String> {
        let provider = Arc::new(Self::new(config));
        builder.register_bearer::<GoogleCloudTarget, Self>(provider)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_account_credentials_deserialization() {
        let json = serde_json::json!({
            "project_id": "test-project",
            "private_key_id": "key123",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIIE...\n-----END PRIVATE KEY-----\n",
            "client_email": "bot@test-project.iam.gserviceaccount.com",
            "client_id": "1234567890",
            "token_uri": "https://oauth2.googleapis.com/token"
        });

        let creds: GoogleServiceAccountCredentials =
            serde_json::from_value(json).unwrap_or_else(|e| panic!("Deserialization failed: {e}"));
        assert_eq!(creds.project_id, "test-project");
        assert_eq!(creds.client_email, "bot@test-project.iam.gserviceaccount.com");
    }

    #[test]
    fn test_config_with_service_account_key_string() {
        let json = serde_json::json!({
            "service_account_key": "{\"project_id\":\"test-project\",\"private_key\":\"pem\",\"client_email\":\"a@b.com\"}"
        });

        let config: GoogleCredentialsConfig =
            serde_json::from_value(json).unwrap_or_else(|e| panic!("Deserialization failed: {e}"));
        let provider = GoogleCredentials::new(config);
        let creds = provider
            .extract_service_account_credentials()
            .unwrap_or_else(|e| panic!("Extraction failed: {e}"))
            .unwrap_or_else(|| panic!("Expected service account credentials"));
        assert_eq!(creds.project_id, "test-project");
        assert_eq!(creds.client_email, "a@b.com");
    }
}
