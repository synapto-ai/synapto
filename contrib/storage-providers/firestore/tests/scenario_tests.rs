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

async fn test_bundle() {
    Synapto::<DotEnv, synapto_storage_firestore::FirestoreStorage>::run::<(
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
