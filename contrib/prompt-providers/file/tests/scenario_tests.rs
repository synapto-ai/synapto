#![allow(unsafe_code)]
#![allow(unused_imports)]

use synapto::Synapto;
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};

async fn test_bundle() {
    type TestConfig = (synapto::config::DotEnv, synapto::config::Env);
    type TestStorage =
        synapto_test::local_storage::LocalStorage<synapto_test::ephemeral_datadir::EphemeralDir>;
    // Use real prompt provider!
    type TestPrompt = prompt_file::FilePromptProvider<synapto_test::ephemeral_datadir::EphemeralDir>;

    Synapto::<TestConfig, TestStorage, TestPrompt>::run::<(
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
