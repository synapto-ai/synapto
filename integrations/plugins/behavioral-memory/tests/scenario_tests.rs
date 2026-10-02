#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(clippy::disallowed_methods)]

use synapto::Synapto;
use synapto::config::ConfigJson;
use synapto::config::{DotEnv, Env};
use synapto_plugin_behavioral_memory::BehavioralMemoryPlugin;
use synapto_test::test_datadir::{ScenarioTestDir, WorkspaceTestDir};
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin, MockLlm,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, TestStorage, run_scenario,
};

// Global Test Bundle Definition
async fn test_bundle() {
    Synapto::builder()
        .configs::<(
            ConfigJson<ScenarioTestDir>,
            ConfigJson<WorkspaceTestDir>,
            DotEnv,
            Env,
        )>()
        .storage::<TestStorage>()
        .llm::<MockLlm>()
        .plugins::<(
            MockAudioInputPlugin,
            MockDocumentsPlugin,
            MockChatPlugin,
            MockSlowReadPlugin,
            MockTtsPlugin,
            MockSttPlugin,
            MockDiarizationPlugin,
            BehavioralMemoryPlugin<TestStorage>,
        )>()
        .run()
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
