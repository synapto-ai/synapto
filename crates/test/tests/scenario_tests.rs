#![allow(clippy::disallowed_methods)]

use synapto::Synapto;
use synapto::config::ConfigJson;
use synapto::config::{DotEnv, Env};
use synapto_test::ephemeral_datadir::EphemeralDir;
use synapto_test::local_storage::LocalStorage;
use synapto_test::test_datadir::WorkspaceTestDir;
use synapto_test::{
    MockAudioInputPlugin, MockChainedToolsPlugin, MockChatPlugin, MockDiarizationPlugin,
    MockDocumentsPlugin, MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};

// Global Test Bundle Definition
async fn test_bundle() {
    Synapto::builder()
        .configs::<(ConfigJson<WorkspaceTestDir>, DotEnv, Env)>()
        .storage::<LocalStorage<EphemeralDir>>()
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
