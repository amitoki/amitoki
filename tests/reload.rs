use amitoki::{
    config::AppConfig,
    control::{ControlServer, Reloader},
    engine::{EngineConfig, EngineSettings},
    firewall::{Firewall, Policy},
    network::PacketIo,
    pipeline::{PipelineConnections, PipelineEngine},
    plugin_manager::PluginStore,
    plugins::builtin_plugins,
};
use async_trait::async_trait;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[path = "support/package.rs"]
mod package_fixture;

struct IdleNetwork;
#[async_trait]
impl PacketIo for IdleNetwork {
    async fn receive(&self, _buffer: &mut [u8]) -> io::Result<usize> {
        std::future::pending().await
    }
    async fn send(&self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }
}

struct Lab {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: PluginStore,
    engine: Arc<PipelineEngine>,
    config: AppConfig,
}
impl Lab {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("amitoki.toml");
        std::fs::write(&path, Self::configuration("pass")).unwrap();
        let package = directory.path().join("package");
        package_fixture::write_package(Path::new(env!("CARGO_BIN_EXE_amitoki-test-block")), &package);
        let store = PluginStore {
            directory: directory.path().join("plugins"),
        };
        store.install(&package, false).unwrap();
        let config = AppConfig::load(&path).unwrap();
        let connections = config.pipeline.as_ref().unwrap().connect((&store, &builtin_plugins().unwrap()), config.context()).await.unwrap();
        let settings = EngineSettings {
            config: EngineConfig::default(),
            firewall: Firewall {
                policy: Policy::Blacklist,
                rules: vec![],
            },
        };
        let engine = Arc::new(PipelineEngine::new(connections, Arc::new(IdleNetwork), settings).unwrap());
        Self {
            _directory: directory,
            path,
            store,
            engine,
            config,
        }
    }
    fn configuration(mode: &str) -> String {
        format!("node_id='a'\nchannel='lab'\ninterface='fake0'\n[engine]\noperation_timeout_ms=1000\n[pipeline]\nroutes=[{{from='capture',to=['inspect']}},{{from='inspect.pass',to=[]}}]\n[[pipeline.stages]]\nid='inspect'\nplugin='block-fixture'\n[pipeline.stages.options]\nmode='{mode}'\n")
    }
    fn reloader(&self) -> Reloader {
        Reloader::new(
            amitoki::control::ReloadSource {
                path: self.path.clone(),
                baseline: self.config.clone(),
                store: self.store.clone(),
            },
            Some(self.engine.clone()),
        )
    }
}

#[tokio::test]
async fn invalid_configuration_failed_initialization_and_timeout_keep_the_old_generation() {
    let lab = Lab::new().await;
    let reload = lab.reloader();
    for text in [
        "invalid".to_owned(),
        Lab::configuration("init-fail"),
        Lab::configuration("init-hang"),
        Lab::configuration("pass").replace("fake0", "other0"),
        Lab::configuration("pass").replace("to=[]", "to=['inspect']"),
    ] {
        std::fs::write(&lab.path, text).unwrap();
        assert!(reload.reload().await.is_err());
        assert_eq!(lab.engine.generation(), 1);
    }
    std::fs::write(&lab.path, Lab::configuration("drop")).unwrap();
    assert_eq!(reload.reload().await.unwrap(), 2);
}

#[tokio::test]
async fn the_reload_command_reports_success_and_failure_without_restarting_the_engine() {
    let lab = Lab::new().await;
    let server = ControlServer::bind(&lab.path).unwrap();
    assert!(ControlServer::bind(&lab.path).is_err());
    let stop = CancellationToken::new();
    let controlling = server.run(lab.reloader(), stop.clone());
    let client = async {
        let command = || {
            let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_amitoki"));
            command.arg("reload").arg("--config").arg(&lab.path);
            command
        };
        let success = command().output().await.unwrap();
        assert!(success.status.success(), "{}", String::from_utf8_lossy(&success.stderr));
        assert!(String::from_utf8_lossy(&success.stdout).contains("generation=2"));
        lab.engine.metrics.captured.store(17, std::sync::atomic::Ordering::Relaxed);
        let observed = amitoki::control::status(&lab.path).await.unwrap();
        assert_eq!(observed.generation, Some(2));
        assert_eq!(observed.captured, 17);
        assert_eq!(observed.topology.stages[0].id, "inspect");
        assert!(!serde_json::to_string(&observed).unwrap().contains("options"));
        std::fs::write(&lab.path, Lab::configuration("init-fail")).unwrap();
        let failure = command().output().await.unwrap();
        assert!(!failure.status.success());
        assert_eq!(lab.engine.generation(), 2);
        assert_eq!(amitoki::control::status(&lab.path).await.unwrap().generation, Some(2));
        std::fs::write(&lab.path, Lab::configuration("pass").replace("inspect", "changed")).unwrap();
        assert!(command().output().await.unwrap().status.success());
        let observed = amitoki::control::status(&lab.path).await.unwrap();
        assert_eq!(observed.generation, Some(3));
        assert_eq!(observed.topology.stages[0].id, "changed");
        stop.cancel();
    };
    let (outcome, ()) = tokio::join!(controlling, client);
    outcome.unwrap();
}
