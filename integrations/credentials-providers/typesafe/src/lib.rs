use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::credentials::{
    CredentialTarget, CredentialsBuilder, PluggableCredentialsProvider, ProvideApiKey,
};
use synapto_interface::secrets::Secret;

/// Target descriptor for resolving TypeSafe API keys via CredentialsHandle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TypeSafeCredentialTarget;

impl CredentialTarget for TypeSafeCredentialTarget {}

/// Explicit credentials configuration for TypeSafe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeSafeCredentialsConfig {
    #[serde(default)]
    pub api_key: Secret<String>,
}

impl Default for TypeSafeCredentialsConfig {
    fn default() -> Self {
        Self {
            api_key: Secret::new(String::new()),
        }
    }
}

/// Pluggable credentials provider for TypeSafe API keys.
pub struct TypeSafeCredentials {
    config: TypeSafeCredentialsConfig,
}

impl TypeSafeCredentials {
    pub fn new(config: TypeSafeCredentialsConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ProvideApiKey<TypeSafeCredentialTarget> for TypeSafeCredentials {
    async fn resolve_api_key(
        &self,
        _target: &TypeSafeCredentialTarget,
    ) -> Result<Secret<String>, String> {
        if self.config.api_key.expose_secret().is_empty() {
            return Err("TypeSafe API key is empty. Configure 'SYNAPTO__CREDENTIALS__synapto_credentials_typesafe__TypeSafeCredentials__api_key' or set 'credentials.synapto_credentials_typesafe.TypeSafeCredentials.api_key' in configuration.".to_string());
        }
        Ok(self.config.api_key.clone())
    }
}

impl PluggableCredentialsProvider for TypeSafeCredentials {
    type Config = TypeSafeCredentialsConfig;

    fn register_provider(
        config: Self::Config,
        builder: &mut CredentialsBuilder,
    ) -> Result<(), String> {
        let provider = Arc::new(Self::new(config));
        builder.register_api_key::<TypeSafeCredentialTarget, Self>(provider)?;
        Ok(())
    }
}
