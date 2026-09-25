use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use genai::resolver::{AuthData, AuthResolver};
use serde::{Deserialize, Serialize};
use synapto_credentials_google::GoogleCloudTarget;
use synapto_interface::credentials::CredentialsHandle;
use synapto_interface::llm::{LlmProvider, RawLlmExecutor, RawLlmOptions};

/// Explicit configuration struct for Google Gemini / Vertex AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleLlmConfig {
    pub google_project_id: Option<String>,
    pub google_vertex_ai_location: Option<String>,
    pub gemini_api_key: Option<synapto_interface::secrets::Secret<String>>,
}

pub struct GoogleLlmExecutor {
    client: genai::Client,
}

impl GoogleLlmExecutor {
    pub fn new(config: GoogleLlmConfig, credentials: CredentialsHandle) -> Self {
        let location = config.google_vertex_ai_location;
        let project_id = config.google_project_id;
        let gemini_api_key = config.gemini_api_key;
        let creds_handle = credentials.clone();

        let auth_resolver = if let (Some(location), Some(project_id)) = (location, project_id) {
            AuthResolver::from_resolver_async_fn(
                move |model: genai::ModelIden| -> std::pin::Pin<
                    Box<
                        dyn Future<Output = Result<Option<AuthData>, genai::resolver::Error>>
                            + Send
                            + 'static,
                    >,
                > {
                    let project_id = project_id.clone();
                    let location = location.clone();
                    let creds_handle = creds_handle.clone();
                    Box::pin(async move {
                        let target = GoogleCloudTarget {
                            scopes: vec![
                                "https://www.googleapis.com/auth/cloud-platform".to_string(),
                            ],
                        };
                        let token = creds_handle
                            .resolve_bearer_token(&target)
                            .await
                            .map_err(genai::resolver::Error::Custom)?;

                        let url = if location == "global" {
                            format!(
                                "https://aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                                project_id, location, model.model_name
                            )
                        } else {
                            format!(
                                "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                                location, project_id, location, model.model_name
                            )
                        };

                        let auth_value = format!("Bearer {}", token.expose_secret());
                        let auth_header = genai::Headers::from(("Authorization", auth_value));
                        Ok(Some(AuthData::RequestOverride {
                            headers: auth_header,
                            url,
                        }))
                    })
                },
            )
        } else if let Some(key) = gemini_api_key {
            AuthResolver::from_resolver_async_fn(
                move |_model: genai::ModelIden| -> std::pin::Pin<
                    Box<
                        dyn Future<Output = Result<Option<AuthData>, genai::resolver::Error>>
                            + Send
                            + 'static,
                    >,
                > {
                    let key = key.clone();
                    Box::pin(
                        async move { Ok(Some(AuthData::Key(key.expose_secret().to_string()))) },
                    )
                },
            )
        } else {
            panic!(
                "GoogleLlm requires either (google_project_id and google_vertex_ai_location) or gemini_api_key"
            );
        };

        let client_config = genai::ClientConfig::default().with_auth_resolver(auth_resolver);
        let client = genai::Client::builder().with_config(client_config).build();

        Self { client }
    }
}

#[async_trait]
impl RawLlmExecutor for GoogleLlmExecutor {
    async fn execute_raw(
        &self,
        model: &str,
        system_prompt: &str,
        prompt: &str,
        options: RawLlmOptions,
    ) -> Result<genai::chat::ChatResponse, String> {
        let mut chat_req = if prompt.is_empty() {
            genai::chat::ChatRequest::default()
        } else {
            genai::chat::ChatRequest::from_user(prompt.to_string())
        };
        if !system_prompt.is_empty() {
            chat_req = chat_req.with_system(system_prompt.to_string());
        }

        if let Some(messages) = options.messages.clone() {
            for msg in messages {
                chat_req = chat_req.append_message(msg);
            }
        }

        let mut chat_options = genai::chat::ChatOptions::default()
            .with_temperature(0.0)
            .with_top_p(0.95);

        if let Some(ref effort) = options.reasoning_effort {
            chat_options = chat_options.with_reasoning_effort(effort.clone());
        }

        if let Some(ref output_schema) = options.output_schema {
            chat_options =
                chat_options.with_response_format(genai::chat::ChatResponseFormat::JsonSpec(
                    genai::chat::JsonSpec::new("schema", output_schema.clone()),
                ));
        }

        if let Some(tools) = options.tools
            && !tools.is_empty()
        {
            chat_req = chat_req.with_tools(tools);
        }

        if let Some(resolved_tools) = options.resolved_tools
            && !resolved_tools.is_empty()
        {
            let mut tool_calls = Vec::new();
            let mut thought_signatures = Vec::new();
            for (call, _) in &resolved_tools {
                if let Some(signatures) = &call.thought_signatures {
                    thought_signatures.extend(signatures.clone());
                }
                tool_calls.push(call.clone());
            }
            chat_req = chat_req.append_message(
                genai::chat::ChatMessage::assistant_tool_calls_with_thoughts(
                    tool_calls,
                    thought_signatures,
                ),
            );

            for (call, response) in resolved_tools {
                chat_req = chat_req.append_message(genai::chat::ChatMessage::from(
                    genai::chat::ToolResponse::new(call.call_id.clone(), response.clone()),
                ));
            }
        }

        let mut retry_count = 0;

        let response_obj = loop {
            match self
                .client
                .exec_chat(model, chat_req.clone(), Some(&chat_options))
                .await
            {
                Ok(val) => {
                    break val;
                }
                Err(err) => {
                    let err_msg = format!("{:?}", err);
                    if retry_count < 5
                        && (err_msg.contains("503")
                            || err_msg.contains("UNAVAILABLE")
                            || err_msg.contains("high demand"))
                    {
                        retry_count += 1;
                        tracing::warn!(?err, "Retry {}/5: Gemini high demand (503)", retry_count);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    tracing::error!(?err, "Failed");
                    return Err(format!("Model call failed: {:?}", err));
                }
            }
        };

        Ok(response_obj)
    }
}

pub struct GoogleLlm {
    executor: Arc<GoogleLlmExecutor>,
}

impl LlmProvider for GoogleLlm {
    type Config = GoogleLlmConfig;

    fn init(config: Self::Config, credentials: CredentialsHandle) -> Result<Self, String> {
        let executor = Arc::new(GoogleLlmExecutor::new(config, credentials));
        Ok(Self { executor })
    }

    fn raw_llm_executor(&self) -> Arc<dyn RawLlmExecutor> {
        self.executor.clone()
    }
}
