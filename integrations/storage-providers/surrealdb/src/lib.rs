use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use synapto_interface::data_dir::DataDirProvider;
use synapto_interface::storage::{
    EmptyStorageConfig, FileStore, KeyValueStore, RecordStore, SortOrder, StorageConnection,
    StorageProviderPool, VectorStore,
};

/// A factory that produces isolated database connections for plugins.
struct SurrealProvider<P: DataDirProvider> {
    // The underlying engine instance (e.g. the SurrealKv database).
    db_engine: Arc<Surreal<Db>>,
    _marker: PhantomData<P>,
}

impl<P: DataDirProvider> StorageProviderPool for SurrealProvider<P> {}

impl<P: DataDirProvider> SurrealProvider<P> {
    async fn new() -> Result<Self, String> {
        let db_dir = P::get_data_dir();
        std::fs::create_dir_all(&db_dir)
            .map_err(|e| format!("Failed to create data directory for SurrealDB: {}", e))?;
        let db_path = db_dir.join("memory.sdb");
        let db_engine = Surreal::new::<surrealdb::engine::local::SurrealKv>(db_path)
            .await
            .map_err(|e| format!("Failed to initialize SurrealDB: {}", e))?;

        Ok(Self {
            db_engine: Arc::new(db_engine),
            _marker: PhantomData,
        })
    }

    async fn connection_for(&self, plugin_namespace: &str) -> Result<SurrealStorage<P>, String> {
        let scoped_db = (*self.db_engine).clone();
        let safe_namespace = plugin_namespace
            .replace("::", "_")
            .replace("<", "_")
            .replace(">", "_")
            .replace(" ", "");

        scoped_db
            .use_ns("ai")
            .use_db(&safe_namespace)
            .await
            .map_err(|e| format!("Failed to set DB scope for {}: {}", safe_namespace, e))?;

        Ok(SurrealStorage {
            db: scoped_db,
            _marker: PhantomData,
        })
    }
}

pub struct SurrealStorage<P: DataDirProvider> {
    db: Surreal<Db>,
    _marker: PhantomData<P>,
}

impl<P: DataDirProvider> SurrealStorage<P> {
    #[cfg(test)]
    pub fn new_with_db(db: Surreal<Db>) -> Self {
        Self {
            db,
            _marker: PhantomData,
        }
    }
}

#[async_trait]
impl<P: DataDirProvider> StorageConnection for SurrealStorage<P> {
    type Config = EmptyStorageConfig;

    async fn connect(
        _config: Self::Config,
        storage_handle: &synapto_interface::storage::StorageHandle,
        plugin_namespace: &str,
    ) -> Result<Self, String> {
        let provider = storage_handle
            .get_or_init_pool(|| async move { SurrealProvider::<P>::new().await })
            .await?;

        provider.connection_for(plugin_namespace).await
    }
}

#[async_trait]
impl<P: DataDirProvider> RecordStore for SurrealStorage<P> {
    async fn upsert_record<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        // SurrealDB syntax for a specific key
        self.db
            .upsert::<Option<serde::de::IgnoredAny>>((collection, key))
            .content(value)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn get_ordered_records<T>(
        &self,
        collection: &str,
        limit: Option<usize>,
        order: SortOrder,
    ) -> Result<Vec<(String, T)>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        // We assume keys are naturally ordered.
        let order_str = match order {
            SortOrder::Ascending => "ASC",
            SortOrder::Descending => "DESC",
        };
        let limit_clause = if let Some(limit) = limit {
            format!("LIMIT {}", limit)
        } else {
            String::new()
        };

        let query = format!(
            "SELECT id, * FROM type::table($table) ORDER BY id {} {}",
            order_str, limit_clause
        );
        let mut response = self
            .db
            .query(query)
            .bind(("table", collection.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let mut results: Vec<(String, T)> = Vec::new();
        let items: Vec<surrealdb::sql::Object> = response.take(0).map_err(|e| e.to_string())?;

        for mut obj in items {
            let id = obj
                .remove("id")
                .and_then(|v| {
                    if let surrealdb::sql::Value::Thing(thing) = v {
                        Some(thing.id.to_raw())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            let val = serde_json::to_value(&obj).map_err(|e| e.to_string())?;
            if let Ok(parsed) = serde_json::from_value::<T>(val) {
                results.push((id, parsed));
            }
        }
        Ok(results)
    }

    async fn delete_record(&self, collection: &str, key: &str) -> Result<(), String> {
        self.db
            .delete::<Option<serde::de::IgnoredAny>>((collection, key))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn trim_records_before(&self, collection: &str, cutoff_key: &str) -> Result<(), String> {
        let query = "DELETE FROM type::table($table) WHERE id < type::thing($table, $cutoff)";
        self.db
            .query(query)
            .bind(("table", collection.to_string()))
            .bind(("cutoff", cutoff_key.to_string()))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[async_trait]
impl<P: DataDirProvider> KeyValueStore for SurrealStorage<P> {
    async fn set<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        self.db
            .upsert::<Option<serde::de::IgnoredAny>>((collection, key))
            .content(value)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn get<T>(&self, collection: &str, key: &str) -> Result<Option<T>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        self.db
            .select((collection, key))
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete(&self, collection: &str, key: &str) -> Result<(), String> {
        self.db
            .delete::<Option<surrealdb::sql::Value>>((collection, key))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn get_all<T>(&self, collection: &str) -> Result<Vec<T>, String>
    where
        T: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        self.db.select(collection).await.map_err(|e| e.to_string())
    }
}

#[async_trait]
impl<P: DataDirProvider> FileStore for SurrealStorage<P> {
    async fn save_file(
        &self,
        collection: &str,
        file_id: &str,
        content: Vec<u8>,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        struct Record {
            file: Vec<u8>,
        }

        self.db
            .upsert::<Option<serde::de::IgnoredAny>>((collection, file_id))
            .content(Record { file: content })
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn get_file(&self, collection: &str, file_id: &str) -> Result<Option<Vec<u8>>, String> {
        #[derive(Deserialize)]
        struct Record {
            file: Vec<u8>,
        }

        self.db
            .select::<Option<Record>>((collection, file_id))
            .await
            .map(|res| res.map(|r| r.file))
            .map_err(|e| e.to_string())
    }

    async fn delete_file(&self, collection: &str, file_id: &str) -> Result<(), String> {
        self.db
            .delete::<Option<serde::de::IgnoredAny>>((collection, file_id))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl<P: DataDirProvider> VectorStore for SurrealStorage<P> {
    async fn setup_collection(&self, collection: &str, dimension: u32) -> Result<(), String> {
        let query = format!(
            "DEFINE TABLE {collection} SCHEMALESS;
            DEFINE FIELD embedding ON {collection} TYPE array<float>;
            DEFINE INDEX {collection}_embedding ON {collection} FIELDS embedding MTREE DIMENSION {dimension} DIST COSINE;"
        );

        self.db
            .query(query)
            .await
            .map_err(|e| format!("Failed to setup vector index on {}: {}", collection, e))?;

        Ok(())
    }

    async fn insert_vectors<T>(&self, collection: &str, records: Vec<T>) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let query = format!("INSERT INTO {} $records;", collection);
        let mut res = self
            .db
            .query(query)
            .bind(("records", records))
            .await
            .map_err(|e| e.to_string())?;

        res.take::<Vec<serde::de::IgnoredAny>>(0)
            .map(|_| ())
            .map_err(|e| e.to_string())
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
        let query = format!(
            "SELECT *, vector::similarity::cosine(embedding, $vector) AS sim FROM {} WHERE embedding != NONE ORDER BY sim DESC LIMIT {}",
            collection, limit
        );
        let mut res = self
            .db
            .query(query)
            .bind(("vector", vector))
            .await
            .map_err(|e| e.to_string())?;

        res.take(0).map_err(|e| e.to_string())
    }

    async fn delete_vectors(
        &self,
        collection: &str,
        filter_field: &str,
        filter_value: &str,
    ) -> Result<(), String> {
        let query = format!(
            "DELETE FROM {} WHERE {} = $filter_value",
            collection, filter_field
        );
        self.db
            .query(query)
            .bind(("filter_value", filter_value.to_string()))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use surrealdb::engine::local::Mem;

    struct TestDir;
    impl DataDirProvider for TestDir {
        fn get_data_dir() -> std::path::PathBuf {
            std::path::PathBuf::from("/tmp/test_surreal")
        }
    }

    #[tokio::test]
    async fn test_file_storage() {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();
        let storage = SurrealStorage::<TestDir>::new_with_db(db);

        let collection = "documents";
        let file_id = "test_doc";
        let content = b"hello surreal world".to_vec();

        // Should return None initially
        assert!(
            storage
                .get_file(collection, file_id)
                .await
                .unwrap()
                .is_none()
        );

        // Save file
        storage
            .save_file(collection, file_id, content.clone())
            .await
            .unwrap();

        // Get file
        let retrieved = storage
            .get_file(collection, file_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(content, retrieved);

        // Delete file
        storage.delete_file(collection, file_id).await.unwrap();
        assert!(
            storage
                .get_file(collection, file_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_vector_storage() {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();
        let storage = SurrealStorage::<TestDir>::new_with_db(db);

        storage
            .setup_collection("test_collection", 768)
            .await
            .unwrap();
    }
}
