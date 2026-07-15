use crate::plugin::Plugin;
use crate::sync::mpsc;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
pub enum DocumentIngestionPolicy {
    #[doc = " Save the raw document only, do not attempt to parse or extract text."]
    Store,
    #[doc = " Save the raw document and run active parsers (e.g. PDF parser) to extract UTF-8 text."]
    StoreAndParse,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
pub struct DocumentRegistrationRequest {
    pub original_filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub policy: DocumentIngestionPolicy,
}

pub struct AddDocumentRequest {
    pub request: DocumentRegistrationRequest,
    pub reply_tx: tokio::sync::oneshot::Sender<DocumentId>,
}

#[doc = " A unique identifier for a document resource."]
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Debug,
    Clone,
    derive_more :: Display,
    derive_more :: From,
    derive_more :: Deref,
)]
pub struct DocumentId(pub String);

#[async_trait]
pub trait DocumentsPlugin: Plugin + Send + Sync {
    async fn start(
        &self,
        add_document_rx: mpsc::Receiver<crate::document::AddDocumentRequest>,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait DocumentProviderPlugin: Plugin + Send + Sync {
    async fn start_document_provider(
        &self,
        add_document_tx: mpsc::Sender<crate::document::AddDocumentRequest>,
    ) -> Result<(), String>;
}
