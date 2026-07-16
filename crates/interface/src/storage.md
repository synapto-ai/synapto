## Persistent Storage

### When to Use It

Use one of the Storage interfaces (`RecordStore`, `KeyValueStore`, `FileStore`, `VectorStore`) when your plugin needs to persist lists of data across reboots (e.g., historical conversation summaries, behavioral insights, or cached configurations).

### Storage Providers

The AI architecture defines storage capabilities as generic traits in `synapto_interface::storage`. The bundle initializing your plugin will inject a concrete storage provider at compile time.

There are currently two available providers:

1. **`storage-local` (`LocalStorage`)**: A zero-dependency, human-readable file backend that writes JSON arrays directly to the plugin's namespace directory. Ideal for small, localized deployments or testing.
2. **`storage-surrealdb` (`SurrealStorage`)**: A full database backend for heavy, high-frequency logging or complex querying.

## Storage Heterogeneity and Connection Pooling

The Synapto architecture supports unlimited heterogeneous storage providers in the same bundle while ensuring connection pooling and strict data isolation.

1. **Shared Config and Pools**: Plugins requesting the exact same storage provider type (e.g., `FirestoreStorage`) share the exact same underlying DB connection pool (via the `StorageRegistry` using `TypeId`) and inherit the same configuration block.
2. **Namespace Isolation**: While the TCP connection pool is shared, the `StorageConnection::connect` method injects the unique `plugin_namespace` into the returned wrapper struct. Data writes are thus intrinsically scoped (e.g., to `/namespaces/plugin_a/` vs `/namespaces/plugin_b/`).
3. **Heterogeneous Resolution**: Plugins requesting different storage providers (e.g., Plugin A asks for `FirestoreStorage`, Plugin B asks for `LocalStorage`) trigger independent config resolution via the `StorageConfigResolver`. Their respective connection pools are instantiated completely independently in the `StorageRegistry` hash map. For details on configuration routing via environment variables or JSON files, see the [Config Providers documentation](../config_providers.md).

### How to Use It (Example)

Define your plugin with a generic type `S` bound to `CollectionStore + StorageConnection`, and inject it via the bundle's `main.rs`.

```rust,ignore
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use synapto_interface::plugin::{Plugin, PluginInitContext, PluginRegistry};
use synapto_interface::storage::{RecordStore, StorageConnection};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Insight {
    pub content: String,
}

pub struct MyMemoryPlugin<S: RecordStore + StorageConnection> {
    store: Arc<S>,
}

#[async_trait]
impl<S: RecordStore + StorageConnection> Plugin for MyMemoryPlugin<S> {
    async fn create(context: &PluginInitContext<'_>) -> Result<Self, String> {
        // Here the configuration and the storage pool are resolved automatically 
        // using the underlying storage type `S`.
        let store = Arc::new(context.store::<S>().await?);
        Ok(Self { store })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        // Register your components (e.g., interaction observer, context provider) here
    }
}

impl<S: RecordStore + StorageConnection> MyMemoryPlugin<S> {
    pub async fn add_insight(&self, insight: String) {
        let doc = Insight { content: insight };
        let id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = self.store.upsert_record("insights", &id, doc).await {
            tracing::error!("Failed to save insight: {}", e);
        }
    }
}
```

In the bundle `main.rs`, you explicitly define the storage provider as `S` in the `Synapto::run` method:

```rust,ignore
// Defining `storage_local::LocalStorage` as the concrete type for `S`
Synapto::<(DotEnv, Env), storage_local::LocalStorage>::run::<(
    MyMemoryPlugin<storage_local::LocalStorage>,
)>().await;
```