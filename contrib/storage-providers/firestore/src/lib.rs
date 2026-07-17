use async_trait::async_trait;
use firestore::{FirestoreDb, FirestoreFindNearestDistanceMeasure, FirestoreVector, FirestoreQueryDirection, FirestoreQueryOrder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::storage::{
    FileStore, KeyValueStore, RecordStore, StorageConnection, VectorStore,
};
use tokio_stream::StreamExt;

#[derive(Deserialize, Debug, Clone)]
pub struct FirestoreConfig {
    pub project_id: String,
    pub credentials_path: Option<String>,
}

struct FirestoreProvider {
    db: Arc<FirestoreDb>,
}
impl synapto_interface::storage::StorageProviderPool for FirestoreProvider {}

#[derive(Clone)]
pub struct FirestoreStorage {
    db: Arc<FirestoreDb>,
    plugin_namespace: String,
}

impl FirestoreStorage {
    fn parent_path(&self) -> Result<String, String> {
        Ok(self
            .db
            .parent_path("namespaces", &self.plugin_namespace)
            .map_err(|e| e.to_string())?
            .to_string())
    }
}

#[async_trait]
impl StorageConnection for FirestoreStorage {
    type Config = FirestoreConfig;

    async fn connect(
        config: Self::Config,
        storage_registry: std::sync::Arc<synapto_interface::storage::StorageRegistry>,
        plugin_namespace: &str,
    ) -> Result<Self, String> {
        let provider = storage_registry
            .get_or_init::<FirestoreProvider, _, _, String>(|| async move {
                let db = if let Some(path) = config.credentials_path {
                    firestore::FirestoreDb::with_options_service_account_key_file(
                        firestore::FirestoreDbOptions::new(config.project_id),
                        path.into(),
                    )
                    .await
                    .map_err(|e| e.to_string())?
                } else {
                    firestore::FirestoreDb::new(&config.project_id)
                        .await
                        .map_err(|e| e.to_string())?
                };

                Ok(FirestoreProvider { db: Arc::new(db) })
            })
            .await?;

        Ok(Self {
            db: provider.db.clone(),
            plugin_namespace: plugin_namespace.to_string(),
        })
    }
}

#[async_trait]
impl RecordStore for FirestoreStorage {
    async fn upsert_record<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        let val = serde_json::to_value(&value).map_err(|e| e.to_string())?;
        self.db
            .fluent()
            .update()
            .in_col(collection)
            .document_id(key)
            .parent(&parent)
            .object(&val)
            .execute::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn get_ordered_records<T>(
        &self,
        collection: &str,
        limit: Option<usize>,
        order: synapto_interface::storage::SortOrder,
    ) -> Result<Vec<(String, T)>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        let direction = match order {
            synapto_interface::storage::SortOrder::Descending => FirestoreQueryDirection::Descending,
            synapto_interface::storage::SortOrder::Ascending => FirestoreQueryDirection::Ascending,
        };

        let mut builder = self
            .db
            .fluent()
            .select()
            .from(collection)
            .parent(&parent)
            .order_by([FirestoreQueryOrder::new(
                firestore::path!(firestore::FirestoreDocument::name),
                direction,
            )]);

        if let Some(limit) = limit {
            builder = builder.limit(limit as u32);
        }

        let stream = builder
            .stream_query_with_metadata()
            .await
            .map_err(|e| e.to_string())?;

        let mut stream = Box::pin(stream);
        let mut results = Vec::new();

        while let Some(item_res) = stream.next().await {
            match item_res {
                Ok(item) => {
                    if let Some(doc) = item.document {
                        let id = doc.name.split('/').last().unwrap_or_default().to_string();
                        if let Ok(obj) = firestore::FirestoreDb::deserialize_doc_to::<T>(&doc) {
                            results.push((id, obj));
                        }
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(results)
    }

    async fn delete_record(&self, collection: &str, key: &str) -> Result<(), String> {
        let parent = self.parent_path()?;
        self.db
            .fluent()
            .delete()
            .from(collection)
            .document_id(key)
            .parent(&parent)
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn trim_records_before(&self, collection: &str, cutoff_key: &str) -> Result<(), String> {
        // Find documents with ID < cutoff_key
        let parent = self.parent_path()?;
        let stream = self
            .db
            .fluent()
            .select()
            .from(collection)
            .parent(&parent)
            .filter(|q| q.for_all([q.field(firestore::path!(firestore::FirestoreDocument::name)).less_than(cutoff_key)]))
            .stream_query_with_metadata()
            .await
            .map_err(|e| e.to_string())?;

        let mut stream = Box::pin(stream);
        while let Some(item_res) = stream.next().await {
            if let Ok(item) = item_res {
                if let Some(doc) = item.document {
                    let id = doc.name.split('/').last().unwrap_or_default();
                    let _ = self.delete_record(collection, id).await;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl KeyValueStore for FirestoreStorage {
    async fn set<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        let val = serde_json::to_value(&value).map_err(|e| e.to_string())?;
        self.db
            .fluent()
            .update()
            .in_col(collection)
            .document_id(key)
            .parent(&parent)
            .object(&val)
            .execute::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn get<T>(&self, collection: &str, key: &str) -> Result<Option<T>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        self.db
            .fluent()
            .select()
            .by_id_in(collection)
            .parent(&parent)
            .obj::<T>()
            .one(key)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete(&self, collection: &str, key: &str) -> Result<(), String> {
        let parent = self.parent_path()?;
        self.db
            .fluent()
            .delete()
            .from(collection)
            .document_id(key)
            .parent(&parent)
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn get_all<T>(&self, collection: &str) -> Result<Vec<T>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        let stream = self
            .db
            .fluent()
            .list()
            .from(collection)
            .parent(&parent)
            .obj::<T>()
            .stream_all()
            .await
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        let mut stream = Box::pin(stream);
        while let Some(item) = stream.next().await {
            results.push(item);
        }
        Ok(results)
    }
}

#[async_trait]
impl FileStore for FirestoreStorage {
    async fn save_file(
        &self,
        collection: &str,
        file_id: &str,
        content: Vec<u8>,
    ) -> Result<(), String> {
        #[derive(Serialize, Deserialize)]
        struct Record {
            file: Vec<u8>,
        }
        let parent = self.parent_path()?;
        self.db
            .fluent()
            .update()
            .in_col(collection)
            .document_id(file_id)
            .parent(&parent)
            .object(&Record { file: content })
            .execute::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn get_file(&self, collection: &str, file_id: &str) -> Result<Option<Vec<u8>>, String> {
        #[derive(Deserialize)]
        struct Record {
            file: Vec<u8>,
        }
        let parent = self.parent_path()?;
        let record = self
            .db
            .fluent()
            .select()
            .by_id_in(collection)
            .parent(&parent)
            .obj::<Record>()
            .one(file_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(record.map(|r| r.file))
    }

    async fn delete_file(&self, collection: &str, file_id: &str) -> Result<(), String> {
        let parent = self.parent_path()?;
        self.db
            .fluent()
            .delete()
            .from(collection)
            .document_id(file_id)
            .parent(&parent)
            .execute()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[async_trait]
impl VectorStore for FirestoreStorage {
    // Using default implementation since Firestore natively handles vector indexing.

    async fn insert_vectors<T>(&self, collection: &str, records: Vec<T>) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        for record in records {
            let key = uuid::Uuid::new_v4().to_string();
            self.upsert_record(collection, &key, record).await?;
        }
        Ok(())
    }

    async fn search_vectors<T>(
        &self,
        collection: &str,
        vector: Vec<f32>,
        limit: u32,
    ) -> Result<Vec<T>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let parent = self.parent_path()?;
        let query_vector = FirestoreVector(vector.into_iter().map(|f| f as f64).collect());

        let stream = self
            .db
            .fluent()
            .select()
            .from(collection)
            .parent(&parent)
            .find_nearest(
                "embedding", // In our ecosystem, embedding fields are consistently named "embedding"
                query_vector,
                FirestoreFindNearestDistanceMeasure::Cosine,
                limit,
            )
            .obj::<T>()
            .stream_query()
            .await
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        let mut stream = Box::pin(stream);
        while let Some(item) = stream.next().await {
            results.push(item);
        }
        Ok(results)
    }

    async fn delete_vectors(
        &self,
        collection: &str,
        filter_field: &str,
        filter_value: &str,
    ) -> Result<(), String> {
        let parent = self.parent_path()?;
        let stream = self
            .db
            .fluent()
            .select()
            .from(collection)
            .parent(&parent)
            .filter(|q| q.for_all([q.field(filter_field).eq(filter_value)]))
            .stream_query()
            .await
            .map_err(|e| e.to_string())?;

        let mut stream = Box::pin(stream);
        while let Some(doc) = stream.next().await {
            if let Some(id) = doc.name.split('/').next_back() {
                self.db
                    .fluent()
                    .delete()
                    .from(collection)
                    .document_id(id)
                    .parent(&parent)
                    .execute()
                    .await
                    .inspect_err(|e| {
                        tracing::error!("Failed to delete Firestore vector document: {:?}", e)
                    })
                    .ok();
            }
        }
        Ok(())
    }
}
