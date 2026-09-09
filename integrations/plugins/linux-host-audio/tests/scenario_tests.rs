#![allow(clippy::disallowed_methods)]

use synapto::Synapto;
use synapto::config::ConfigJson;
use synapto::config::{DotEnv, Env};

use synapto_plugin_linux_host_audio::HostAudioInputPlugin;
use synapto_test::ephemeral_datadir::EphemeralDir;
use synapto_test::local_storage::LocalStorage;
use synapto_test::test_datadir::WorkspaceTestDir;
use synapto_test::{
    MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin, MockSlowReadPlugin, MockSttPlugin,
    MockTtsPlugin, run_scenario,
};

async fn test_bundle() {
    Synapto::<(ConfigJson<WorkspaceTestDir>, DotEnv, Env), LocalStorage<EphemeralDir>>::run::<(
        MockDocumentsPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
        HostAudioInputPlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
