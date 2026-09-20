mod distillation;
mod store;

pub(crate) use store::{
    ActiveWorkingMemory, WorkingMemoryEntry, WorkingMemoryProvider, WorkingMemoryStore,
};

use synapto_interface::{
    context::EngineRegistries,
    interaction::ObservedInteraction,
    llm::{LlmExecutor, ModelConfig},
    sync::mpsc,
};

pub(crate) fn start(
    observers_tx: &mut Vec<mpsc::Sender<ObservedInteraction>>,
    registries: EngineRegistries,
    llm_executor: LlmExecutor,
    model_config: ModelConfig,
) -> WorkingMemoryStore {
    let store = WorkingMemoryStore::new();
    let provider = WorkingMemoryProvider::new(store.clone());
    registries.context.current.register(provider);

    let (observer_tx, observer_rx) = mpsc::channel(100);
    observers_tx.push(observer_tx);

    tokio::spawn(distillation::distillation_task(
        observer_rx,
        store.clone(),
        llm_executor,
        model_config,
    ));

    store
}
