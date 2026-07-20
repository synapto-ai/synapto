
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
use synapto_plugin_mumble::MumblePlugin;
use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt, core::IntoContainerPort};

async fn test_bundle() {
    Synapto::<(ConfigJson<ScenarioTestDir>, DotEnv, Env), LocalStorage<EphemeralDir>>::run::<(
        MockDocumentsPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
        MumblePlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    let _container = GenericImage::new("docker.io/mumblevoip/mumble-server", "latest")
        .with_env_var("MUMBLE_SUPERUSER_PASSWORD", "Test")
        .with_mapped_port(64738, 64738.tcp())
        .with_mapped_port(64738, 64738.udp())
        .start()
        .await
        .expect("Failed to start mumble testcontainer");

    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}
