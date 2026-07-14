#![doc = include_str!("storage.md")]

use async_trait::async_trait;

/// A marker trait for safe DB connection pooling
pub trait StorageProviderPool: Send + Sync + 'static {}

#[derive(Default)]
pub struct StorageRegistry {
    map: tokio::sync::Mutex<
        std::collections::HashMap<
            std::any::TypeId,
            std::sync::Arc<dyn std::any::Any + Send + Sync>,
        >,
    >,
}

impl StorageRegistry {
    /// Lazily initializes a global shared resource. If the resource already exists,
    /// it is returned immediately. This allows multiple plugins to safely share a
    /// single connection pool without requiring manual initialization in main.rs.
    pub async fn get_or_init<T: StorageProviderPool, F, Fut, E>(
        &self,
        init: F,
    ) -> Result<std::sync::Arc<T>, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let mut map = self.map.lock().await;
        let type_id = std::any::TypeId::of::<T>();

        if let Some(resource) = map.get(&type_id) {
            // This can only fail if the TypeId of T doesn't match the Arc's inner type,
            // which is impossible since we keyed the HashMap by TypeId::of::<T>().
            return Ok(resource
                .clone()
                .downcast::<T>()
                .unwrap_or_else(|_| unreachable!("TypeId mismatch in StorageRegistry")));
        }

        let resource = std::sync::Arc::new(init().await?);
        map.insert(type_id, resource.clone());
        Ok(resource)
    }
}

pub trait StorageConfigResolver: Send + Sync + 'static {
    fn resolve_config(
        &self,
        crate_name: &str,
        storage_type_name: &str,
    ) -> Option<serde_json::Value>;
}

/// The entry point for a generic storage adapter.
/// It guarantees that plugins can seamlessly initialize their underlying connection
/// using the shared StorageRegistry without requiring manual setup in main.rs.
#[async_trait]
pub trait StorageConnection: Send + Sync + Sized + 'static {
    type Config: serde::de::DeserializeOwned + Send + Sync;

    async fn connect(
        config: Self::Config,
        storage_registry: std::sync::Arc<StorageRegistry>,
        plugin_namespace: &str,
    ) -> Result<Self, String>;
}

use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmptyStorageConfig {}

#[async_trait]
pub trait RecordStore: Send + Sync + 'static {
    /// Inserts or updates an individual record.
    /// If the key is a Timestamp or ULID, time-based ordering is natively maintained.
    async fn upsert_record<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static;

    /// Retrieves records guaranteed to be sorted by their key.
    /// Allows pagination to avoid loading the entire history into RAM.
    async fn get_ordered_records<T>(
        &self,
        collection: &str,
        limit: Option<usize>,
        reverse: bool,
    ) -> Result<Vec<(String, T)>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static;

    /// Deletes a specific record.
    async fn delete_record(&self, collection: &str, key: &str) -> Result<(), String>;

    /// Atomically deletes all records with a key smaller than `cutoff_key`.
    /// This natively delegates sliding-window "VecDeque::pop_front()" operations to the DB.
    async fn trim_records_before(&self, collection: &str, cutoff_key: &str) -> Result<(), String>;
}

/// For storing and retrieving items by a unique string ID.
#[async_trait]
pub trait KeyValueStore: Send + Sync + 'static {
    async fn set<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static;

    async fn get<T>(&self, collection: &str, key: &str) -> Result<Option<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static;

    async fn delete(&self, collection: &str, key: &str) -> Result<(), String>;

    async fn get_all<T>(&self, collection: &str) -> Result<Vec<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static;
}

/// Trait for storing, retrieving, and deleting raw binary files.
#[async_trait]
pub trait FileStore: Send + Sync + 'static {
    /// Saves raw bytes under the specified collection and unique file identifier.
    async fn save_file(
        &self,
        collection: &str,
        file_id: &str,
        content: Vec<u8>,
    ) -> Result<(), String>;

    /// Retrieves raw bytes by its identifier.
    async fn get_file(&self, collection: &str, file_id: &str) -> Result<Option<Vec<u8>>, String>;

    /// Deletes a file.
    async fn delete_file(&self, collection: &str, file_id: &str) -> Result<(), String>;
}
#[async_trait]
pub trait VectorStore: Send + Sync + 'static {
    /// Ensures a collection is ready for vector operations.
    ///
    /// This method is typically called during the application boot sequence or when a service starts up.
    /// It should be idempotent.
    ///
    /// Depending on the underlying database, this method might:
    /// - Define a schema or table if it doesn't exist.
    /// - Create necessary vector search indexes (e.g., M-Tree, HNSW).
    /// - Do absolutely nothing if the database manages indexing transparently (e.g., Firestore).
    ///
    /// By default, this does nothing and returns `Ok(())`. Storage providers that require
    /// explicit schema or index definition must override this implementation.
    async fn setup_collection(&self, _collection: &str, _dimension: u32) -> Result<(), String> {
        Ok(())
    }

    async fn insert_vectors<T>(&self, collection: &str, records: Vec<T>) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static;

    async fn search_vectors<T>(
        &self,
        collection: &str,
        vector: Vec<f32>,
        limit: u32,
    ) -> Result<Vec<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static;

    async fn delete_vectors(
        &self,
        collection: &str,
        filter_field: &str,
        filter_value: &str,
    ) -> Result<(), String>;
}
