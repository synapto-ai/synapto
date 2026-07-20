
use synapto::Synapto;
use synapto::config::{DotEnv, Env};
use synapto::config::ConfigJson;
use synapto_test::test_datadir::ScenarioTestDir;
use synapto_test::local_storage::LocalStorage;
use synapto_test::ephemeral_datadir::EphemeralDir;
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};
use synapto_plugin_stt_speechmatics::SttSpeechmaticsPlugin;

async fn test_bundle() {
    Synapto::<(ConfigJson<ScenarioTestDir>, DotEnv, Env), LocalStorage<EphemeralDir>>::run::<(
        MockAudioInputPlugin,
        MockDocumentsPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
        SttSpeechmaticsPlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
