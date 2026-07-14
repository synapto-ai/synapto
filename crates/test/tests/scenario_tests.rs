#![allow(clippy::disallowed_methods)]

use synapto::Synapto;
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};

// Global Test Bundle Definition
async fn test_bundle() {
    Synapto::<
        (synapto::config::DotEnv, synapto::config::Env),
        synapto_storage_local::LocalStorageProvider<synapto_datadir_ephemeral::EphemeralDir>,
    >::run::<(
        MockAudioInputPlugin,
        MockDocumentsPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
    )>()
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_tool_dual_channel() {
    run_scenario(
        "scenarios/async-tool-dual-channel/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_tool_reaction() {
    run_scenario("scenarios/async-tool-reaction/scenario.yaml", test_bundle).await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn behavioral_memory_test() {
    run_scenario(
        "scenarios/behavioral-memory-test/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edge_cases_test() {
    run_scenario("scenarios/edge-cases-test/scenario.yaml", test_bundle).await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_assert_test() {
    run_scenario("scenarios/multi-assert-test/scenario.yaml", test_bundle).await;
}
