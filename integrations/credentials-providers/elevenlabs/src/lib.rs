use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use synapto_interface::credentials::{
    CredentialsBuilder, PluggableCredentialsProvider, ProvideApiKey,
};
use synapto_interface::secrets::Secret;

/// Target descriptor for ElevenLabs authentication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ElevenLabsTarget;

impl synapto_interface::credentials::CredentialTarget for ElevenLabsTarget {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElevenLabsCredentialsConfig {
    #[serde(default)]
    pub api_key: Option<Secret<String>>,
}

pub struct ElevenLabsCredentials {
    config: ElevenLabsCredentialsConfig,
}

impl ElevenLabsCredentials {
    pub fn new(config: ElevenLabsCredentialsConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ProvideApiKey<ElevenLabsTarget> for ElevenLabsCredentials {
    async fn resolve_api_key(
        &self,
        _target: &ElevenLabsTarget,
    ) -> Result<Secret<String>, String> {
        if let Some(ref key) = self.config.api_key
            && !key.expose_secret().is_empty()
        {
            return Ok(key.clone());
        }
        if let Ok(env_key) = std::env::var("ELEVENLABS_API_KEY")
            && !env_key.is_empty()
        {
            return Ok(Secret::new(env_key));
        }
        if let Ok(env_key) = std::env::var("ELEVEN_API_KEY")
            && !env_key.is_empty()
        {
            return Ok(Secret::new(env_key));
        }
        Err("ElevenLabs API key is not configured. Set 'credentials.elevenlabs.ElevenLabsCredentials.api_key' in configuration or ELEVENLABS_API_KEY environment variable.".to_string())
    }
}

impl PluggableCredentialsProvider for ElevenLabsCredentials {
    type Config = ElevenLabsCredentialsConfig;

    fn register_provider(
        config: Self::Config,
        builder: &mut CredentialsBuilder,
    ) -> Result<(), String> {
        let provider = Arc::new(Self::new(config));
        builder.register_api_key::<ElevenLabsTarget, Self>(provider)?;
        Ok(())
    }
}
