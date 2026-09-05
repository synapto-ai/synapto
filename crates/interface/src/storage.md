## Persistent Storage

### When to Use It

Use the storage interfaces when a plugin must persist data across restarts (for example: conversation history, behavioral memory, files, or vector embeddings).

### Available Storage Capabilities

The storage system exposes four capability traits in `synapto_interface::storage`:

1. **`RecordStore`**: Ordered record persistence with upsert, retrieval by key order (ascending or descending), single-item deletion, and sliding-window cutoff trimming (`trim_records_before`).
2. **`KeyValueStore`**: Basic key-value operations (`get`, `set`, `delete`, `get_all`) within a collection.
3. **`FileStore`**: Binary blob storage (`save_file`, `get_file`, `delete_file`) within a collection.
4. **`VectorStore`**: Vector index preparation (`setup_collection`), vector batch insertion (`insert_vectors`), similarity search (`search_vectors`), and filter deletion (`delete_vectors`).

---

## Storage Architecture

### Connection Pooling (`StorageRegistry`)

Multiple plugins requesting the same storage provider share a single underlying connection pool via `StorageRegistry`. The registry caches instances by their `TypeId`.

### Namespace Isolation

Storage implementations must isolate data for each plugin using the `plugin_namespace` string provided during connection initialization.

### Storage Configuration Resolution

During plugin initialization (`context.store::<S>()`), the runtime invokes `StorageConfigResolver`. It parses the configuration block matching the crate and storage struct names into `S::Config`.

---

## Using Storage in a Plugin

Define the plugin with a generic type parameter `S` bounded by the required store traits and `StorageConnection`:

```rust,ignore
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use synapto_interface::plugin::{Plugin, PluginInitContext, PluginRegistry};
use synapto_interface::storage::{RecordStore, StorageConnection};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryItem {
    pub content: String,
}

pub struct MemoryPlugin<S: RecordStore + StorageConnection> {
    store: Arc<S>,
}

#[async_trait]
impl<S: RecordStore + StorageConnection> Plugin for MemoryPlugin<S> {
    async fn create(context: &PluginInitContext<'_>) -> Result<Self, String> {
        let store = Arc::new(context.store::<S>().await?);
        Ok(Self { store })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, _registry: &mut R) {}
}

impl<S: RecordStore + StorageConnection> MemoryPlugin<S> {
    pub async fn record(&self, key: &str, content: String) -> Result<(), String> {
        self.store
            .upsert_record("memories", key, MemoryItem { content })
            .await
    }
}
```

---

## Creating a Custom Storage Provider

To create a new storage provider, implement:
1. `StorageProviderPool` on your shared connection or client instance.
2. `StorageConnection` on your per-plugin scoped handle.
3. One or more store traits (`RecordStore`, `KeyValueStore`, `FileStore`, `VectorStore`).

### Local File-Backed vs Remote Storage

- **Local Storage Providers**: If the provider writes to local disk, parameterize the struct with `P: DataDirProvider` to receive the data directory at compile time. Use `type Config = EmptyStorageConfig;`.
- **Remote Storage Providers**: If the provider connects to a network database, define a custom configuration struct deriving `Deserialize` (e.g. `MyDbConfig`) and set `type Config = MyDbConfig;`.

### Step-by-Step Implementation Example

```rust,ignore
use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use std::sync::Arc;
use synapto_interface::data_dir::DataDirProvider;
use synapto_interface::storage::{
    EmptyStorageConfig, KeyValueStore, RecordStore, SortOrder,
    StorageConnection, StorageProviderPool, StorageRegistry,
};

// 1. Shared connection pool / client instance
struct CustomDbClient {
    // Database client / connection pool handle
}

impl StorageProviderPool for CustomDbClient {}

// 2. Per-plugin scoped storage handle
pub struct CustomStorage<P: DataDirProvider> {
    client: Arc<CustomDbClient>,
    namespace: String,
    _marker: PhantomData<P>,
}

// 3. Connection lifecycle implementation
#[async_trait]
impl<P: DataDirProvider> StorageConnection for CustomStorage<P> {
    type Config = EmptyStorageConfig;

    async fn connect(
        _config: Self::Config,
        storage_registry: Arc<StorageRegistry>,
        plugin_namespace: &str,
    ) -> Result<Self, String> {
        let base_path = P::get_data_dir();
        
        let client = storage_registry
            .get_or_init::<CustomDbClient, _, _, String>(|| async move {
                // Initialize the database client once
                Ok(CustomDbClient {})
            })
            .await?;

        Ok(Self {
            client,
            namespace: plugin_namespace.to_string(),
            _marker: PhantomData,
        })
    }
}

// 4. Implement required storage capability traits
#[async_trait]
impl<P: DataDirProvider> KeyValueStore for CustomStorage<P> {
    async fn set<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        // Save value scoped to self.namespace and collection
        Ok(())
    }

    async fn get<T>(&self, collection: &str, key: &str) -> Result<Option<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        // Retrieve value
        Ok(None)
    }

    async fn delete(&self, collection: &str, key: &str) -> Result<(), String> {
        // Delete key
        Ok(())
    }

    async fn get_all<T>(&self, collection: &str) -> Result<Vec<T>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        // List all items
        Ok(Vec::new())
    }
}

#[async_trait]
impl<P: DataDirProvider> RecordStore for CustomStorage<P> {
    async fn upsert_record<T>(&self, collection: &str, key: &str, value: T) -> Result<(), String>
    where
        T: Serialize + Send + Sync + 'static,
    {
        Ok(())
    }

    async fn get_ordered_records<T>(
        &self,
        collection: &str,
        limit: Option<usize>,
        order: SortOrder,
    ) -> Result<Vec<(String, T)>, String>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        Ok(Vec::new())
    }

    async fn delete_record(&self, collection: &str, key: &str) -> Result<(), String> {
        Ok(())
    }

    async fn trim_records_before(&self, collection: &str, cutoff_key: &str) -> Result<(), String> {
        Ok(())
    }
}
```
