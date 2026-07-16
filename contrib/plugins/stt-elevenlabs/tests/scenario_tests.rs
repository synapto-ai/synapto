#![allow(unsafe_code)]
#![allow(unused_imports)]

use synapto::Synapto;
use synapto::config::DotEnv;
use synapto_test::local_storage::LocalStorage;
use synapto_test::ephemeral_datadir::EphemeralDir;
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};
use synapto_plugin_stt_elevenlabs::SttElevenLabsPlugin;

async fn test_bundle() {
    Synapto::<DotEnv, LocalStorage<EphemeralDir>>::run::<(
        MockAudioInputPlugin,
        MockDocumentsPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
        SttElevenLabsPlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
