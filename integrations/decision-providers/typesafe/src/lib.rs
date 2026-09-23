use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use synapto_credentials_typesafe::TypeSafeCredentialTarget;
use synapto_interface::credentials::CredentialsHandle;
use synapto_interface::decision::{
    DecisionAnswer, DecisionProvider, DecisionQuestion, RawDecisionExecutor,
};

/// Explicit configuration struct for TypeSafe decision provider.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TypeSafeDecisionConfig {
    pub model: String,
    pub api_endpoint: String,
}

#[derive(Serialize)]
struct SystemOneRequest<'a> {
    state: serde_json::Value,
    model: &'a str,
    questions: BTreeMap<String, DecisionQuestion>,
}

#[derive(Deserialize)]
struct SystemOneResponse {
    #[allow(unused)]
    pub model: String,
    pub answers: BTreeMap<String, DecisionAnswer>,
}

pub struct TypeSafeDecisionExecutor {
    client: reqwest::Client,
    config: TypeSafeDecisionConfig,
    credentials: CredentialsHandle,
}

impl TypeSafeDecisionExecutor {
    pub fn new(config: TypeSafeDecisionConfig, credentials: CredentialsHandle) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            credentials,
        }
    }
}

#[async_trait]
impl RawDecisionExecutor for TypeSafeDecisionExecutor {
    async fn evaluate_raw(
        &self,
        model: Option<&str>,
        state: serde_json::Value,
        questions: BTreeMap<String, DecisionQuestion>,
    ) -> Result<BTreeMap<String, DecisionAnswer>, String> {
        let active_model = model.unwrap_or(&self.config.model);

        let api_key = self
            .credentials
            .resolve_api_key(&TypeSafeCredentialTarget)
            .await
            .map_err(|e| format!("TypeSafe authentication failed: {}", e))?;

        let request_payload = SystemOneRequest {
            state,
            model: active_model,
            questions,
        };

        let response = self
            .client
            .post(&self.config.api_endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", api_key.expose_secret()))
            .json(&request_payload)
            .send()
            .await
            .map_err(|e| format!("TypeSafe HTTP request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!(
                "TypeSafe HTTP error {}: {}",
                status.as_u16(),
                error_text
            ));
        }

        let parsed = response
            .json::<SystemOneResponse>()
            .await
            .map_err(|e| format!("Failed to parse TypeSafe response: {}", e))?;

        Ok(parsed.answers)
    }
}

pub struct TypeSafeDecision {
    executor: Arc<TypeSafeDecisionExecutor>,
}

impl DecisionProvider for TypeSafeDecision {
    type Config = TypeSafeDecisionConfig;

    fn init(config: Self::Config, credentials: CredentialsHandle) -> Result<Self, String>
    where
        Self: Sized,
    {
        let executor = Arc::new(TypeSafeDecisionExecutor::new(config, credentials));
        Ok(Self { executor })
    }

    fn raw_decision_executor(&self) -> Arc<dyn RawDecisionExecutor> {
        self.executor.clone()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use synapto_interface::decision::{NoulCriteria, NoulQuestion};

    #[test]
    fn test_serialization_request() {
        let mut questions = BTreeMap::new();
        questions.insert(
            "q1".to_string(),
            DecisionQuestion::Noul(NoulQuestion {
                instructions: "Is this urgent?".to_string(),
                criteria: Some(NoulCriteria {
                    r#true: "Time-sensitive".to_string(),
                    r#false: "Not time-sensitive".to_string(),
                }),
            }),
        );

        let req = SystemOneRequest {
            state: serde_json::json!({ "text": "hello" }),
            model: "jev-latest",
            questions,
        };

        let json = serde_json::to_string(&req).expect("Serialization failed");
        assert!(json.contains("\"type\":\"noul\""));
        assert!(json.contains("Is this urgent?"));
    }
}
