use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct ReadDocumentRequest {
    pub target_document_id: crate::types::DocumentId,
    pub reply_tx: tokio::sync::oneshot::Sender<Result<String, String>>,
}

#[derive(Debug)]
pub struct ReadUrlRequest {
    pub url: String,
    pub reply_tx: tokio::sync::oneshot::Sender<Result<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: crate::types::DocumentId,
    pub name: String,
    pub summary: Option<String>,
}
