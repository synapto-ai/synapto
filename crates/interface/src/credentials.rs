use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::secrets::Secret;

/// Domain marker trait for credential target descriptors.
pub trait CredentialTarget: Send + Sync + 'static {}

/// Capability trait for scoped bearer tokens (e.g. Google Cloud, AWS STS, Azure AD, OAuth2).
#[async_trait]
pub trait ProvideBearerToken<Target: CredentialTarget>: Send + Sync + 'static {
    async fn resolve_bearer_token(&self, target: &Target) -> Result<Secret<String>, String>;
}

/// Capability trait for API keys (e.g. Speechmatics, OpenAI, Anthropic, ElevenLabs).
#[async_trait]
pub trait ProvideApiKey<Target: CredentialTarget>: Send + Sync + 'static {
    async fn resolve_api_key(&self, target: &Target) -> Result<Secret<String>, String>;
}

/// Capability trait for basic authentication (username and secret password).
#[async_trait]
pub trait ProvideBasicAuth<Target: CredentialTarget>: Send + Sync + 'static {
    async fn resolve_basic_auth(&self, target: &Target)
    -> Result<(String, Secret<String>), String>;
}

#[async_trait]
trait ErasedBearerResolver: Send + Sync + 'static {
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<Secret<String>, String>;
}

struct TypedBearerResolver<Target, P> {
    provider: Arc<P>,
    _marker: std::marker::PhantomData<Target>,
}

#[async_trait]
impl<Target: CredentialTarget, P: ProvideBearerToken<Target>> ErasedBearerResolver
    for TypedBearerResolver<Target, P>
{
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<Secret<String>, String> {
        let typed_target = target.downcast_ref::<Target>().ok_or_else(|| {
            format!(
                "Invalid target type: expected {}",
                std::any::type_name::<Target>()
            )
        })?;
        self.provider.resolve_bearer_token(typed_target).await
    }
}

#[async_trait]
trait ErasedApiKeyResolver: Send + Sync + 'static {
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<Secret<String>, String>;
}

struct TypedApiKeyResolver<Target, P> {
    provider: Arc<P>,
    _marker: std::marker::PhantomData<Target>,
}

#[async_trait]
impl<Target: CredentialTarget, P: ProvideApiKey<Target>> ErasedApiKeyResolver
    for TypedApiKeyResolver<Target, P>
{
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<Secret<String>, String> {
        let typed_target = target.downcast_ref::<Target>().ok_or_else(|| {
            format!(
                "Invalid target type: expected {}",
                std::any::type_name::<Target>()
            )
        })?;
        self.provider.resolve_api_key(typed_target).await
    }
}

#[async_trait]
trait ErasedBasicAuthResolver: Send + Sync + 'static {
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<(String, Secret<String>), String>;
}

struct TypedBasicAuthResolver<Target, P> {
    provider: Arc<P>,
    _marker: std::marker::PhantomData<Target>,
}

#[async_trait]
impl<Target: CredentialTarget, P: ProvideBasicAuth<Target>> ErasedBasicAuthResolver
    for TypedBasicAuthResolver<Target, P>
{
    async fn resolve_erased(
        &self,
        target: &(dyn std::any::Any + Send + Sync),
    ) -> Result<(String, Secret<String>), String> {
        let typed_target = target.downcast_ref::<Target>().ok_or_else(|| {
            format!(
                "Invalid target type: expected {}",
                std::any::type_name::<Target>()
            )
        })?;
        self.provider.resolve_basic_auth(typed_target).await
    }
}

/// Builder used by credential providers to register their typed resolvers.
#[derive(Default)]
pub struct CredentialsBuilder {
    bearer_resolvers: HashMap<TypeId, Arc<dyn ErasedBearerResolver>>,
    api_key_resolvers: HashMap<TypeId, Arc<dyn ErasedApiKeyResolver>>,
    basic_auth_resolvers: HashMap<TypeId, Arc<dyn ErasedBasicAuthResolver>>,
}

impl CredentialsBuilder {
    pub fn register_bearer<Target: CredentialTarget, P: ProvideBearerToken<Target>>(
        &mut self,
        provider: Arc<P>,
    ) -> Result<(), String> {
        let type_id = TypeId::of::<Target>();
        if self.bearer_resolvers.contains_key(&type_id) {
            return Err(format!(
                "Credentials resolver already registered for target: {}",
                std::any::type_name::<Target>()
            ));
        }
        let resolver = TypedBearerResolver {
            provider,
            _marker: std::marker::PhantomData,
        };
        self.bearer_resolvers.insert(type_id, Arc::new(resolver));
        Ok(())
    }

    pub fn register_api_key<Target: CredentialTarget, P: ProvideApiKey<Target>>(
        &mut self,
        provider: Arc<P>,
    ) -> Result<(), String> {
        let type_id = TypeId::of::<Target>();
        if self.api_key_resolvers.contains_key(&type_id) {
            return Err(format!(
                "Credentials resolver already registered for target: {}",
                std::any::type_name::<Target>()
            ));
        }
        let resolver = TypedApiKeyResolver {
            provider,
            _marker: std::marker::PhantomData,
        };
        self.api_key_resolvers.insert(type_id, Arc::new(resolver));
        Ok(())
    }

    pub fn register_basic_auth<Target: CredentialTarget, P: ProvideBasicAuth<Target>>(
        &mut self,
        provider: Arc<P>,
    ) -> Result<(), String> {
        let type_id = TypeId::of::<Target>();
        if self.basic_auth_resolvers.contains_key(&type_id) {
            return Err(format!(
                "Credentials resolver already registered for target: {}",
                std::any::type_name::<Target>()
            ));
        }
        let resolver = TypedBasicAuthResolver {
            provider,
            _marker: std::marker::PhantomData,
        };
        self.basic_auth_resolvers
            .insert(type_id, Arc::new(resolver));
        Ok(())
    }

    pub fn build(self) -> CredentialsHandle {
        CredentialsHandle {
            bearer_resolvers: Arc::new(self.bearer_resolvers),
            api_key_resolvers: Arc::new(self.api_key_resolvers),
            basic_auth_resolvers: Arc::new(self.basic_auth_resolvers),
        }
    }
}

/// Type-erased registry container for runtime credential resolution.
#[derive(Clone, Default)]
pub struct CredentialsHandle {
    bearer_resolvers: Arc<HashMap<TypeId, Arc<dyn ErasedBearerResolver>>>,
    api_key_resolvers: Arc<HashMap<TypeId, Arc<dyn ErasedApiKeyResolver>>>,
    basic_auth_resolvers: Arc<HashMap<TypeId, Arc<dyn ErasedBasicAuthResolver>>>,
}

impl CredentialsHandle {
    /// Resolves a scoped bearer token against registered providers.
    pub async fn resolve_bearer_token<Target: CredentialTarget>(
        &self,
        target: &Target,
    ) -> Result<Secret<String>, String> {
        let type_id = TypeId::of::<Target>();
        if let Some(resolver) = self.bearer_resolvers.get(&type_id) {
            resolver.resolve_erased(target).await
        } else {
            Err(format!(
                "No registered credentials provider implements ProvideBearerToken<{}>",
                std::any::type_name::<Target>()
            ))
        }
    }

    /// Resolves an API key against registered providers.
    pub async fn resolve_api_key<Target: CredentialTarget>(
        &self,
        target: &Target,
    ) -> Result<Secret<String>, String> {
        let type_id = TypeId::of::<Target>();
        if let Some(resolver) = self.api_key_resolvers.get(&type_id) {
            resolver.resolve_erased(target).await
        } else {
            Err(format!(
                "No registered credentials provider implements ProvideApiKey<{}>",
                std::any::type_name::<Target>()
            ))
        }
    }

    /// Resolves basic authentication against registered providers.
    pub async fn resolve_basic_auth<Target: CredentialTarget>(
        &self,
        target: &Target,
    ) -> Result<(String, Secret<String>), String> {
        let type_id = TypeId::of::<Target>();
        if let Some(resolver) = self.basic_auth_resolvers.get(&type_id) {
            resolver.resolve_erased(target).await
        } else {
            Err(format!(
                "No registered credentials provider implements ProvideBasicAuth<{}>",
                std::any::type_name::<Target>()
            ))
        }
    }
}

/// Factory trait for pluggable credential providers initialized by Synapto.
pub trait PluggableCredentialsProvider: Send + Sync + 'static {
    type Config: serde::de::DeserializeOwned + Default + Send + Sync;

    fn register_provider(
        config: Self::Config,
        builder: &mut CredentialsBuilder,
    ) -> Result<(), String>;
}
