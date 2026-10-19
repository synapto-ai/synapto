#![allow(clippy::disallowed_methods)]

use synapto::Synapto;
use synapto::config::ConfigJson;
use synapto::config::{DotEnv, Env};
use synapto_test::test_datadir::WorkspaceTestDir;
use synapto_test::{
    MockAudioInputPlugin, MockChainedToolsPlugin, MockChatPlugin, MockDiarizationPlugin,
    MockDocumentsPlugin, MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, TestStorage,
    run_scenario,
};

// Global Test Bundle Definition
async fn test_bundle() {
    Synapto::builder()
        .configs::<(ConfigJson<WorkspaceTestDir>, DotEnv, Env)>()
        .storage::<TestStorage>()
        .credentials::<(synapto_credentials_google::GoogleCredentials,)>()
        .llm::<synapto_llm_google::GoogleLlm>()
        .plugins::<(
            MockAudioInputPlugin,
            MockDocumentsPlugin,
            MockChatPlugin,
            MockSlowReadPlugin,
            MockChainedToolsPlugin,
            MockTtsPlugin,
            MockSttPlugin,
            MockDiarizationPlugin,
        )>()
        .run()
        .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_tool_dual_channel() {
    run_scenario(
        "tests/scenarios/async-tool-dual-channel/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_tool_reaction() {
    run_scenario(
        "tests/scenarios/async-tool-reaction/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_working_memory_consequent_turn() {
    run_scenario(
        "tests/scenarios/active-working-memory/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn behavioral_memory_test() {
    run_scenario(
        "tests/scenarios/behavioral-memory-test/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edge_cases_test() {
    run_scenario("tests/scenarios/edge-cases-test/scenario.yaml", test_bundle).await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_assert_test() {
    run_scenario(
        "tests/scenarios/multi-assert-test/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn document_summary_scenario() {
    run_scenario(
        "tests/scenarios/document-summary/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_tool_calling() {
    run_scenario(
        "tests/scenarios/multi-turn-tool-calling/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_tool_stop() {
    run_scenario(
        "tests/scenarios/multi-turn-tool-stop/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_broadcast_lag_resilience_and_direct_audio_wiring() {
    run_scenario(
        "tests/scenarios/2026-10-17-broadcast-lag-resilience-and-direct-audio-wiring/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_tool_turn_barrier_and_working_memory_alignment() {
    run_scenario(
        "tests/scenarios/multi-tool-turn-barrier-and-working-memory-alignment/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_tool_barge_in_race() {
    run_scenario(
        "tests/scenarios/multi-tool-barge-in-race/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_09_20_runtime_stability_and_smells_remediation() {
    run_scenario(
        "tests/scenarios/2026-09-20-runtime-stability-and-smells-remediation/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_deterministic_in_flight_tool_resolution_sync() {
    run_scenario(
        "tests/scenarios/2026-10-17-deterministic-in-flight-tool-resolution-sync/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_episodic_memory_persistence_and_rollout_registration() {
    run_scenario(
        "tests/scenarios/2026-10-17-episodic-memory-persistence-and-rollout-registration/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_semantic_memory_batch_accumulation_and_startup_barrier() {
    run_scenario(
        "tests/scenarios/2026-10-17-semantic-memory-batch-accumulation-and-startup-barrier/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_behavioral_memory_opaque_keys_and_surrealdb_query_binding() {
    run_scenario(
        "tests/scenarios/2026-10-17-behavioral-memory-opaque-keys-and-surrealdb-query-binding/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_llm_schema_ref_inlining_and_boundary_sanitization() {
    run_scenario(
        "tests/scenarios/2026-10-17-llm-schema-ref-inlining-and-boundary-sanitization/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_tool_output_json_sandboxing_and_documents_plugin_fixes() {
    run_scenario(
        "tests/scenarios/2026-10-17-tool-output-json-sandboxing-and-documents-plugin-fixes/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_local_storage_concurrency_locks_and_ephemeral_dir_leak() {
    run_scenario(
        "tests/scenarios/2026-10-17-local-storage-concurrency-locks-and-ephemeral-dir-leak/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_telemetry_reloadable_filter_and_async_gui_error_channel() {
    run_scenario(
        "tests/scenarios/2026-10-17-telemetry-reloadable-filter-and-async-gui-error-channel/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[should_panic(expected = "Scenario Failed")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rfc_2026_10_17_process_lifecycle_browser_cleanup_and_durable_pubsub_ack() {
    run_scenario(
        "tests/scenarios/2026-10-17-process-lifecycle-browser-cleanup-and-durable-pubsub-ack/scenario.yaml",
        test_bundle,
    )
    .await;
}
