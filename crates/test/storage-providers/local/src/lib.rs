use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use synapto_interface::storage::{
    EmptyStorageConfig, FileStore, KeyValueStore, RecordStore, StorageConnection, VectorStore,
};
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

/// A local file-based JSON storage provider.
///
/// Implements `StorageConnection` and `RecordStore`.
/// It stores data by writing full JSON arrays to `.json` files inside the plugin namespace.
pub struct LocalStorageProvider<P: synapto_interface::data_dir::DataDirProvider> {
    base_dir: PathBuf,
    locks: DashMap<String, Arc<RwLock<()>>>,
    _marker: std::marker::PhantomData<P>,
}

impl<P: synapto_interface::data_dir::DataDirProvider> LocalStorageProvider<P> {
    /// Helper to get the file path for a given records collection.
    fn get_records_path(&self, collection: &str) -> PathBuf {
        self.base_dir.join(format!("{}_records.json", collection))
    }

    /// Helper to get or create the lock for a given collection.
    fn get_lock(&self, collection: &str) -> Arc<RwLock<()>> {
        self.locks
            .entry(collection.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// Helper to get the file path for a given kv collection.
    fn get_kv_path(&self, collection: &str) -> PathBuf {
        self.base_dir.join(format!("{}_kv.json", collection))
    }

    /// Helper to perform atomic write to a file
    async fn atomic_write(&self, path: &Path, content: &[u8]) -> Result<(), String> {
        let temp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));

        let mut temp_file = File::create(&temp_path)
            .await
            .map_err(|e| format!("Failed to create temporary file: {}", e))?;

        temp_file.write_all(content).await.map_err(|e| {
            std::fs::remove_file(&temp_path).ok();
            format!("Failed to write to temporary file: {}", e)
        })?;

        temp_file.sync_all().await.map_err(|e| {
            std::fs::remove_file(&temp_path).ok();
            format!("Failed to sync temporary file: {}", e)
        })?;

        fs::rename(&temp_path, path).await.map_err(|e| {
            std::fs::remove_file(&temp_path).ok();
            format!("Failed to commit file atomic rename: {}", e)
        })?;

        Ok(())
    }
}

#[async_trait]
impl<P: synapto_interface::data_dir::DataDirProvider> StorageConnection for LocalStorageProvider<P> {
    type Config = EmptyStorageConfig;

    async fn connect(
        _config: Self::Config,
        _storage_registry: Arc<synapto_interface::storage::StorageRegistry>,
        plugin_namespace: &str,
    ) -> Result<Self, String> {
        let base_dir = P::get_data_dir().join("storage").join(plugin_namespace);

        // Ensure the directory exists
        fs::create_dir_all(&base_dir)
            .await
            .map_err(|e| format!("Failed to create plugin storage directory: {}", e))?;

        Ok(LocalStorageProvider {
            base_dir,
            locks: DashMap::new(),
            _marker: std::marker::PhantomData,
        })
    }
}

#[async_trait]
impl<P: synapto_interface::data_dir::DataDirProvider> RecordStore for LocalStorageProvider<P> {
    async fn upsert_record<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self.get_records_path(collection);
        let mut map: std::collections::BTreeMap<String, serde_json::Value> = if path.exists() {
            let content = fs::read_to_string(&path)
                .await
                .map_err(|e| format!("Failed to read records file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize records: {}", e))?
        } else {
            std::collections::BTreeMap::new()
        };

        map.insert(
            key.to_string(),
            serde_json::to_value(value)
                .map_err(|e| format!("Failed to serialize new record: {}", e))?,
        );

        let serialized = serde_json::to_string_pretty(&map)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        self.atomic_write(&path, serialized.as_bytes()).await?;

        Ok(())
    }

    async fn get_ordered_records<T>(
        &self,
        collection: &str,
        limit: Option<usize>,
        reverse: bool,
    ) -> Result<Vec<(String, T)>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _read_guard = lock.read().await;

        let path = self.get_records_path(collection);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read records file: {}", e))?;

        let map: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize records items: {}", e))?;

        let mut iter: Box<dyn Iterator<Item = _>> = if reverse {
            Box::new(map.into_iter().rev())
        } else {
            Box::new(map.into_iter())
        };

        if let Some(limit) = limit {
            iter = Box::new(iter.take(limit));
        }

        let mut result = Vec::new();
        for (k, v) in iter {
            let item: T = serde_json::from_value(v)
                .map_err(|e| format!("Failed to deserialize record: {}", e))?;
            result.push((k, item));
        }

        Ok(result)
    }

    async fn delete_record(&self, collection: &str, key: &str) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self.get_records_path(collection);
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read records file: {}", e))?;

        let mut map: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize records: {}", e))?;

        if map.remove(key).is_some() {
            let serialized = serde_json::to_string_pretty(&map)
                .map_err(|e| format!("Failed to serialize: {}", e))?;

            self.atomic_write(&path, serialized.as_bytes()).await?;
        }

        Ok(())
    }

    async fn trim_records_before(&self, collection: &str, cutoff_key: &str) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self.get_records_path(collection);
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read records file: {}", e))?;

        let mut map: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize records: {}", e))?;

        let initial_len = map.len();
        map.retain(|k, _| k.as_str() >= cutoff_key);

        if map.len() < initial_len {
            let serialized = serde_json::to_string_pretty(&map)
                .map_err(|e| format!("Failed to serialize: {}", e))?;

            self.atomic_write(&path, serialized.as_bytes()).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl<P: synapto_interface::data_dir::DataDirProvider> KeyValueStore for LocalStorageProvider<P> {
    async fn set<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let lock = self.get_lock(&format!("{}_kv", collection));
        let _write_guard = lock.write().await;

        let path = self.get_kv_path(collection);
        let mut map: std::collections::HashMap<String, serde_json::Value> = if path.exists() {
            let content = fs::read_to_string(&path)
                .await
                .map_err(|e| format!("Failed to read kv collection file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize kv collection: {}", e))?
        } else {
            std::collections::HashMap::new()
        };

        map.insert(
            key.to_string(),
            serde_json::to_value(value)
                .map_err(|e| format!("Failed to serialize new item: {}", e))?,
        );

        let serialized = serde_json::to_string_pretty(&map)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        self.atomic_write(&path, serialized.as_bytes()).await?;

        Ok(())
    }

    async fn get<T>(&self, collection: &str, key: &str) -> Result<Option<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let lock = self.get_lock(&format!("{}_kv", collection));
        let _read_guard = lock.read().await;

        let path = self.get_kv_path(collection);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read kv collection file: {}", e))?;

        let map: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize kv collection: {}", e))?;

        if let Some(value) = map.get(key) {
            Ok(Some(serde_json::from_value(value.clone()).map_err(
                |e| format!("Failed to deserialize item: {}", e),
            )?))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, collection: &str, key: &str) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_kv", collection));
        let _write_guard = lock.write().await;

        let path = self.get_kv_path(collection);
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read kv collection file: {}", e))?;

        let mut map: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize kv collection: {}", e))?;

        map.remove(key);

        let serialized = serde_json::to_string_pretty(&map)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        self.atomic_write(&path, serialized.as_bytes()).await?;

        Ok(())
    }

    async fn get_all<T>(&self, collection: &str) -> Result<Vec<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let lock = self.get_lock(&format!("{}_kv", collection));
        let _read_guard = lock.read().await;

        let path = self.get_kv_path(collection);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read kv collection file: {}", e))?;

        let map: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize kv collection: {}", e))?;

        let mut items = Vec::new();
        for (_, value) in map {
            items.push(
                serde_json::from_value(value)
                    .map_err(|e| format!("Failed to deserialize item: {}", e))?,
            );
        }

        Ok(items)
    }
}

#[async_trait]
impl<P: synapto_interface::data_dir::DataDirProvider> FileStore for LocalStorageProvider<P> {
    async fn save_file(
        &self,
        collection: &str,
        file_id: &str,
        content: Vec<u8>,
    ) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self
            .base_dir
            .join(collection)
            .join(format!("{}.bin", file_id));

        // Ensure collection directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create collection dir: {}", e))?;
        }

        let temp_path = path.with_extension(format!("bin.tmp.{}", uuid::Uuid::new_v4()));

        let mut temp_file = File::create(&temp_path)
            .await
            .map_err(|e| format!("Failed to create temporary file: {}", e))?;

        temp_file.write_all(&content).await.map_err(|e| {
            std::fs::remove_file(&temp_path)
                .inspect_err(|err| {
                    tracing::error!(
                        "Failed to remove temporary file {}: {:?}",
                        temp_path.display(),
                        err
                    )
                })
                .ok();
            format!("Failed to write to temporary file: {}", e)
        })?;

        temp_file.sync_all().await.map_err(|e| {
            std::fs::remove_file(&temp_path)
                .inspect_err(|err| {
                    tracing::error!(
                        "Failed to remove temporary file {}: {:?}",
                        temp_path.display(),
                        err
                    )
                })
                .ok();
            format!("Failed to sync temporary file: {}", e)
        })?;

        fs::rename(&temp_path, &path).await.map_err(|e| {
            std::fs::remove_file(&temp_path)
                .inspect_err(|err| {
                    tracing::error!(
                        "Failed to remove temporary file {}: {:?}",
                        temp_path.display(),
                        err
                    )
                })
                .ok();
            format!("Failed to commit file atomic rename: {}", e)
        })?;

        Ok(())
    }

    async fn get_file(&self, collection: &str, file_id: &str) -> Result<Option<Vec<u8>>, String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _read_guard = lock.read().await;

        let path = self
            .base_dir
            .join(collection)
            .join(format!("{}.bin", file_id));
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read(&path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        Ok(Some(content))
    }

    async fn delete_file(&self, collection: &str, file_id: &str) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self
            .base_dir
            .join(collection)
            .join(format!("{}.bin", file_id));
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| format!("Failed to delete file: {}", e))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distance {
    Euclidean,
    Cosine,
    DotProduct,
}

pub fn get_cache_attr(metric: Distance, vec: &[f32]) -> f32 {
    match metric {
        Distance::DotProduct | Distance::Euclidean => 0.0,
        Distance::Cosine => vec.iter().map(|&x| x.powi(2)).sum::<f32>().sqrt(),
    }
}

pub fn get_distance_fn(metric: Distance) -> impl Fn(&[f32], &[f32], f32) -> f32 {
    match metric {
        Distance::Euclidean => euclidian_distance,
        Distance::Cosine | Distance::DotProduct => dot_product,
    }
}

fn euclidian_distance(a: &[f32], b: &[f32], a_sum_squares: f32) -> f32 {
    let mut cross_terms = 0.0;
    let mut b_sum_squares = 0.0;

    for (i, j) in a.iter().zip(b) {
        cross_terms += i * j;
        b_sum_squares += j.powi(2);
    }

    2.0f32
        .mul_add(-cross_terms, a_sum_squares + b_sum_squares)
        .max(0.0)
        .sqrt()
}

fn dot_product(a: &[f32], b: &[f32], _: f32) -> f32 {
    a.iter().zip(b).fold(0.0, |acc, (x, y)| acc + x * y)
}

pub fn normalize(vec: &[f32]) -> Vec<f32> {
    let magnitude = (vec.iter().fold(0.0, |acc, &val| val.mul_add(val, acc))).sqrt();

    if magnitude > f32::EPSILON {
        vec.iter().map(|&val| val / magnitude).collect()
    } else {
        vec.to_vec()
    }
}

pub struct ScoreIndex {
    pub score: f32,
    pub index: usize,
}

impl PartialEq for ScoreIndex {
    fn eq(&self, other: &Self) -> bool {
        self.score.eq(&other.score)
    }
}

impl Eq for ScoreIndex {}

impl PartialOrd for ScoreIndex {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoreIndex {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

#[async_trait]
impl<P: synapto_interface::data_dir::DataDirProvider> VectorStore for LocalStorageProvider<P> {
    // Using default implementation since LocalStorageProvider doesn't need explicit setup.

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
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let all_records =
            RecordStore::get_ordered_records::<serde_json::Value>(self, collection, None, false)
                .await?;
        let all: Vec<serde_json::Value> = all_records.into_iter().map(|(_, v)| v).collect();

        let metric = Distance::Cosine;
        let memo_attr = get_cache_attr(metric, &vector);
        let distance_fn = get_distance_fn(metric);

        let mut scores = Vec::new();
        for (index, item) in all.iter().enumerate() {
            if let Some(embedding_vec) = item.get("embedding").and_then(|e| e.as_array()) {
                let mut vec_f32 = Vec::new();
                for v in embedding_vec {
                    if let Some(f) = v.as_f64() {
                        vec_f32.push(f as f32);
                    }
                }
                if !vec_f32.is_empty() {
                    let score = distance_fn(&vec_f32, &vector, memo_attr);
                    scores.push(ScoreIndex { score, index });
                }
            }
        }

        let mut heap = std::collections::BinaryHeap::new();
        for score_index in scores {
            let below_limit = heap.len() < limit as usize;
            let higher_than_peek = heap.peek().is_some_and(|peeked| score_index < *peeked);
            if below_limit || higher_than_peek {
                heap.push(score_index);
                if heap.len() > limit as usize {
                    heap.pop();
                }
            }
        }

        let mut results = Vec::new();
        for ScoreIndex { index, .. } in heap.into_sorted_vec() {
            results.push(serde_json::from_value(all[index].clone()).map_err(|e| e.to_string())?);
        }

        Ok(results)
    }

    async fn delete_vectors(
        &self,
        collection: &str,
        filter_field: &str,
        filter_value: &str,
    ) -> Result<(), String> {
        let lock = self.get_lock(&format!("{}_records", collection));
        let _write_guard = lock.write().await;

        let path = self.get_records_path(collection);
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read collection file: {}", e))?;

        let mut map: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to deserialize collection: {}", e))?;

        let initial_len = map.len();
        map.retain(|_, item| {
            let mut keep = true;
            if let Some(field) = item.get(filter_field) {
                if let Some(s) = field.as_str() {
                    if s == filter_value {
                        keep = false;
                    }
                } else if field.is_null() && filter_value == "null" {
                    keep = false;
                }
            }
            keep
        });

        if map.len() < initial_len {
            let serialized = serde_json::to_string_pretty(&map)
                .map_err(|e| format!("Failed to serialize: {}", e))?;
            self.atomic_write(&path, serialized.as_bytes()).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestItem {
        id: u32,
        name: String,
    }

    #[tokio::test]
    async fn test_json_storage_provider() {
        let dir = tempdir().unwrap();
        let provider: LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir> =
            LocalStorageProvider {
                base_dir: dir.path().to_path_buf(),
                locks: DashMap::new(),
                _marker: std::marker::PhantomData,
            };

        let collection = "test_items";

        // Initial get_all should return empty
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&provider, collection, None, false)
                .await
                .unwrap();
        assert!(items.is_empty());

        // Push one item
        let item1 = TestItem {
            id: 1,
            name: "Item 1".to_string(),
        };
        provider
            .upsert_record(collection, "key1", item1.clone())
            .await
            .unwrap();

        // Push another item
        let item2 = TestItem {
            id: 2,
            name: "Item 2".to_string(),
        };
        provider
            .upsert_record(collection, "key2", item2.clone())
            .await
            .unwrap();

        // Get all should return both items
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&provider, collection, None, false)
                .await
                .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1, item1);
        assert_eq!(items[1].1, item2);

        // Delete should remove one item
        provider.delete_record(collection, "key1").await.unwrap();
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&provider, collection, None, false)
                .await
                .unwrap();
        assert_eq!(items.len(), 1);

        // We can upsert after clear
        provider
            .upsert_record(collection, "key1", item1.clone())
            .await
            .unwrap();
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&provider, collection, None, false)
                .await
                .unwrap();
        assert_eq!(items.len(), 2);

        provider
            .trim_records_before(collection, "key2")
            .await
            .unwrap();
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&provider, collection, None, false)
                .await
                .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, item2);
    }

    #[tokio::test]
    async fn test_concurrent_pushes() {
        let dir = tempdir().unwrap();
        let provider: Arc<LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir>> =
            Arc::new(LocalStorageProvider {
                base_dir: dir.path().to_path_buf(),
                locks: DashMap::new(),
                _marker: std::marker::PhantomData,
            });

        let collection = "concurrent_test";
        let mut handles = vec![];

        // Spawn 100 concurrent tasks
        for i in 0..100 {
            let provider_clone = provider.clone();
            let collection_clone = collection.to_string();
            handles.push(tokio::spawn(async move {
                let item = TestItem {
                    id: i,
                    name: format!("Item {}", i),
                };
                provider_clone
                    .upsert_record(&collection_clone, &format!("key_{:03}", i), item)
                    .await
                    .unwrap();
            }));
        }

        // Wait for all to finish
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify no data was lost
        let items: Vec<(String, TestItem)> =
            RecordStore::get_ordered_records(&*provider, collection, None, false)
                .await
                .unwrap();
        assert_eq!(items.len(), 100);

        let mut ids: Vec<u32> = items.iter().map(|item| item.1.id).collect();
        ids.sort();
        let expected: Vec<u32> = (0..100).collect();
        assert_eq!(ids, expected);
    }
    #[tokio::test]
    async fn test_file_storage() {
        let dir = tempdir().unwrap();
        let provider: LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir> =
            LocalStorageProvider {
                base_dir: dir.path().to_path_buf(),
                locks: DashMap::new(),
                _marker: std::marker::PhantomData,
            };

        let collection = "documents";
        let file_id = "test_doc";
        let content = b"hello world".to_vec();

        // Should return None initially
        assert!(
            provider
                .get_file(collection, file_id)
                .await
                .unwrap()
                .is_none()
        );

        // Save file
        provider
            .save_file(collection, file_id, content.clone())
            .await
            .unwrap();

        // Get file
        let retrieved = provider
            .get_file(collection, file_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(content, retrieved);

        // Delete file
        provider.delete_file(collection, file_id).await.unwrap();
        assert!(
            provider
                .get_file(collection, file_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_kv_storage() {
        let dir = tempdir().unwrap();
        let provider: LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir> =
            LocalStorageProvider {
                base_dir: dir.path().to_path_buf(),
                locks: DashMap::new(),
                _marker: std::marker::PhantomData,
            };

        let collection = "config";

        // Initial get should return empty
        let val: Option<TestItem> = provider.get(collection, "my_key").await.unwrap();
        assert!(val.is_none());

        let item = TestItem {
            id: 1,
            name: "KV Item".to_string(),
        };

        // Set
        provider
            .set(collection, "my_key", item.clone())
            .await
            .unwrap();

        // Get
        let retrieved: TestItem = provider.get(collection, "my_key").await.unwrap().unwrap();
        assert_eq!(item, retrieved);

        // Get All
        let all: Vec<TestItem> = KeyValueStore::get_all(&provider, collection).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], item);

        // Delete
        provider.delete(collection, "my_key").await.unwrap();
        let val: Option<TestItem> = provider.get(collection, "my_key").await.unwrap();
        assert!(val.is_none());
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct VecItem {
        id: String,
        embedding: Vec<f32>,
    }

    #[tokio::test]
    async fn test_vector_storage() {
        let dir = tempdir().unwrap();
        let provider: LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir> =
            LocalStorageProvider {
                base_dir: dir.path().to_path_buf(),
                locks: DashMap::new(),
                _marker: std::marker::PhantomData,
            };

        let collection = "vectors";

        provider.setup_collection(collection, 3).await.unwrap();

        let v1 = VecItem {
            id: "a".to_string(),
            embedding: vec![1.0, 0.0, 0.0],
        };
        let v2 = VecItem {
            id: "b".to_string(),
            embedding: vec![0.0, 1.0, 0.0],
        };
        let v3 = VecItem {
            id: "c".to_string(),
            embedding: vec![0.707, 0.707, 0.0],
        };

        provider
            .insert_vectors(collection, vec![v1.clone(), v2.clone(), v3.clone()])
            .await
            .unwrap();

        let query = vec![1.0, 0.0, 0.0];
        let results: Vec<VecItem> = provider.search_vectors(collection, query, 2).await.unwrap();

        // v1 should be exactly 1.0 similarity (most similar)
        // v3 should be ~0.707 similarity
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a");
        assert_eq!(results[1].id, "c");

        provider
            .delete_vectors(collection, "id", "a")
            .await
            .unwrap();

        let results_after: Vec<VecItem> = provider
            .search_vectors(collection, vec![1.0, 0.0, 0.0], 2)
            .await
            .unwrap();
        assert_eq!(results_after.len(), 2);
        assert_eq!(results_after[0].id, "c");
        assert_eq!(results_after[1].id, "b");
    }
}
