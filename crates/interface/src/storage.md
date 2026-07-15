## Persistent Storage with `CollectionStore`

### When to Use It

Use `CollectionStore` when your plugin needs to persist lists of data across reboots (e.g., historical conversation summaries, behavioral insights, or cached configurations).

### Storage Providers

The AI architecture defines storage capabilities as generic traits in `synapto_interface::storage`. The bundle initializing your plugin will inject a concrete storage provider at compile time.

There are currently two available providers:

1. **`storage-local` (`LocalStorage`)**: A zero-dependency, human-readable file backend that writes JSON arrays directly to the plugin's namespace directory. Ideal for small, localized deployments or testing.
2. **`storage-surrealdb` (`SurrealStorage`)**: A full database backend for heavy, high-frequency logging or complex querying.

### How to Use It (Example)

Define your plugin with a generic type `S` bound to `CollectionStore + StorageConnection`, and inject it via the bundle's `main.rs`.

```rust,ignore
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use synapto_interface::storage::{CollectionStore, StorageConnection};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Insight {
    pub content: String,
}

pub struct MyMemoryPlugin<S: CollectionStore + StorageConnection> {
    store: Arc<S>,
}

impl<S: CollectionStore + StorageConnection> MyMemoryPlugin<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    pub async fn add_insight(&self, insight: String) {
        let doc = Insight { content: insight };
        if let Err(e) = self.store.push("insights", doc).await {
            tracing::error!("Failed to save insight: {}", e);
        }
    }
}
```

In the bundle `main.rs`:

```rust,ignore
// Using the lightweight JSON file provider
use storage_local::LocalStorage;

let storage = Arc::new(
    LocalStorage::connect(registries.clone(), &data_dir, "my_plugin_namespace")
        .await
        .unwrap()
);

let my_plugin = MyMemoryPlugin::new(storage);
```