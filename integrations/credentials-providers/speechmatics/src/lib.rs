use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use synapto_interface::credentials::{
    CredentialsBuilder, PluggableCredentialsProvider, ProvideApiKey,
};
use synapto_interface::secrets::Secret;

/// Target descriptor for Speechmatics authentication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SpeechmaticsTarget;

impl synapto_interface::credentials::CredentialTarget for SpeechmaticsTarget {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpeechmaticsCredentialsConfig {
    #[serde(default)]
    pub api_key: Option<Secret<String>>,
}

pub struct SpeechmaticsCredentials {
    config: SpeechmaticsCredentialsConfig,
}

impl SpeechmaticsCredentials {
    pub fn new(config: SpeechmaticsCredentialsConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ProvideApiKey<SpeechmaticsTarget> for SpeechmaticsCredentials {
    async fn resolve_api_key(
        &self,
        _target: &SpeechmaticsTarget,
    ) -> Result<Secret<String>, String> {
        if let Some(ref key) = self.config.api_key
            && !key.expose_secret().is_empty()
        {
            return Ok(key.clone());
        }
        if let Ok(env_key) = std::env::var("SPEECHMATICS_API_KEY")
            && !env_key.is_empty()
        {
            return Ok(Secret::new(env_key));
        }
        Err("Speechmatics API key is not configured. Set 'credentials.speechmatics.SpeechmaticsCredentials.api_key' in configuration or SPEECHMATICS_API_KEY environment variable.".to_string())
    }
}

impl PluggableCredentialsProvider for SpeechmaticsCredentials {
    type Config = SpeechmaticsCredentialsConfig;

    fn register_provider(
        config: Self::Config,
        builder: &mut CredentialsBuilder,
    ) -> Result<(), String> {
        let provider = Arc::new(Self::new(config));
        builder.register_api_key::<SpeechmaticsTarget, Self>(provider)?;
        Ok(())
    }
}
