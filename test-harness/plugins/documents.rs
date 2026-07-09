use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use synapto_interface::context::{ContextProvider, ContextRequest, TemporalScope};
use synapto_interface::document::{
    AddDocumentRequest, DocumentId, DocumentsPlugin as DocumentsPluginTrait,
};
use synapto_interface::llm::LLMSafe;
use synapto_interface::plugin::{Plugin, PluginContext, PluginRegistry};
use synapto_interface::sync::mpsc;
use synapto_interface::tool::Tool;
use tokio::sync::Mutex;

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, LLMSafe)]
pub struct CognitiveLLMAvailableDocument {
    pub id: String,
    pub filename: String,
}

pub struct DocumentStore {
    documents: HashMap<String, (String, String)>, // id -> (filename, content)
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }
}

pub struct DocumentContextProvider {
    store: Arc<Mutex<DocumentStore>>,
}

#[async_trait]
impl ContextProvider for DocumentContextProvider {
    type Context = Vec<CognitiveLLMAvailableDocument>;
    const NAME: &'static str = "available_documents";
    const SCOPE: TemporalScope = TemporalScope::Current;

    async fn context(&self, _request: &ContextRequest) -> Result<Self::Context, String> {
        let store = self.store.lock().await;
        let docs = store
            .documents
            .iter()
            .map(|(id, (filename, _))| CognitiveLLMAvailableDocument {
                id: id.clone(),
                filename: filename.clone(),
            })
            .collect();
        Ok(docs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, LLMSafe)]
pub struct ReadDocumentArgs {
    #[schemars(
        description = "The opaque DocumentId from attached_documents OR available_documents"
    )]
    pub target_document_id: String,
}

pub struct ReadDocumentPluginTool {
    store: Arc<Mutex<DocumentStore>>,
}

#[async_trait]
impl Tool for ReadDocumentPluginTool {
    type Arguments = ReadDocumentArgs;

    const NAME: &'static str = "read_document";
    const DESCRIPTION: &'static str = "Retrieves text from a specific document. Use this tool BEFORE making your final decision if you need to read a file.";

    async fn is_available(
        &self,
        _ctx: &ContextRequest,
        compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        let has_docs = compiled_context
            .get("available_documents")
            .and_then(|arr| arr.as_array())
            .is_some_and(|arr| !arr.is_empty());
        Ok(has_docs)
    }

    async fn execute(
        &self,
        _ctx: &ContextRequest,
        args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        let store = self.store.lock().await;
        if let Some((_, content)) = store.documents.get(&args.target_document_id) {
            Ok(serde_json::Value::String(content.clone()))
        } else {
            Err(format!("Document {} not found", args.target_document_id))
        }
    }
}

pub struct MockDocumentsPlugin {
    store: Arc<Mutex<DocumentStore>>,
}

#[async_trait]
impl Plugin for MockDocumentsPlugin {
    async fn create(_context: PluginContext) -> Result<Self, String> {
        Ok(Self {
            store: Arc::new(Mutex::new(DocumentStore::new())),
        })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_documents(self.clone());

        let provider = Arc::new(DocumentContextProvider {
            store: self.store.clone(),
        });
        registry.register_context_provider(provider);

        registry.register_tool(ReadDocumentPluginTool {
            store: self.store.clone(),
        });
    }
}

#[async_trait]
impl DocumentsPluginTrait for MockDocumentsPlugin {
    async fn start(
        &self,
        mut add_document_rx: mpsc::Receiver<AddDocumentRequest>,
    ) -> Result<(), String> {
        let store = self.store.clone();
        tokio::spawn(async move {
            let mut id_counter = 0;
            while let Some(req) = add_document_rx.recv().await {
                id_counter += 1;
                let doc_id = format!("doc_{}", id_counter);

                let content = String::from_utf8(req.request.data)
                    .unwrap_or_else(|_| "Invalid UTF-8".to_string());

                let mut st = store.lock().await;
                st.documents
                    .insert(doc_id.clone(), (req.request.original_filename, content));

                req.reply_tx.send(DocumentId(doc_id)).ok();
            }
        });
        Ok(())
    }
}
