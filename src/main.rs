use amitoki::pipeline::PipelineConnections;
use amitoki::{
    config::AppConfig,
    engine::{Engine, EngineSettings},
    network::LinuxSocket,
    pipeline::PipelineEngine,
    plugin_manager::{run_cli, PluginStore},
    plugins::builtin_plugins,
    runtime::Runtime,
};
use log::info;
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--version"] || arguments == ["-V"] {
        println!("amitoki {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if arguments.first().map(String::as_str) == Some("plugin") {
        return run_cli(&arguments[1..]).await;
    }
    if arguments.first().map(String::as_str) == Some("reload") {
        return amitoki::control::run_cli(&arguments[1..]).await;
    }
    if arguments.first().map(String::as_str) == Some("debug") {
        return amitoki::debug::run_cli(&arguments[1..]).await;
    }
    if arguments.first().map(String::as_str) == Some("web") {
        return amitoki::web::run_cli(&arguments[1..]).await;
    }
    let registry = builtin_plugins()?;
    let store = PluginStore::from_environment()?;
    if arguments == ["--list-plugins"] {
        println!("{}", registry.names().join("\n"));
        for package in store.list()? {
            println!("{}", package.manifest.name);
        }
        return Ok(());
    }
    if arguments == ["--help"] || arguments == ["-h"] {
        println!("amitoki [--config PATH] [--check-config]\namitoki --list-plugins\namitoki --version\namitoki plugin --help\namitoki debug --help\namitoki reload --help\namitoki web --help");
        return Ok(());
    }
    let mut path = PathBuf::from("amitoki.toml");
    let mut check_only = false;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--config" => path = arguments.next().ok_or("--configにパスを指定してください")?.into(),
            "--check-config" => check_only = true,
            _ => return Err(format!("不明な引数: {argument}").into()),
        }
    }
    let path = amitoki::plugin_manager::expand_path(&path)?.canonicalize()?;
    let config = AppConfig::load(&path)?;
    let baseline = config.clone();
    if let Some(relay) = &config.relay {
        if !registry.names().contains(&relay.plugin.as_str()) {
            store.resolved_options(&relay.plugin, &relay.options)?;
        }
    }
    if let Some(pipeline) = &config.pipeline {
        pipeline.check(&store, &config.context())?;
    }
    if check_only {
        println!("共通設定と外部プラグインの設定は正常です（接続・デバイスは未検証）");
        return Ok(());
    }
    match dotenv::dotenv() {
        Ok(_) => {},
        Err(dotenv::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err(".envを読み込めません（認証情報保護のため内容は表示しません）".into()),
    }
    let control_server = amitoki::control::ControlServer::bind(&path)?;
    let network = Arc::new(LinuxSocket::open(&config.interface, config.promiscuous)?);
    network.set_receive_buffer(config.engine.capture_buffer_bytes)?;
    // 同じUIDの子から/proc経由で本体のNICを開かせない。プラグインのOS隔離とは別の保護。
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let context = config.context();
    let settings = EngineSettings {
        firewall: config.firewall,
        config: config.engine,
    };
    let engine = if let Some(pipeline) = config.pipeline {
        let connections = pipeline.connect((&store, &registry), context).await?;
        Runtime::Pipeline(Arc::new(PipelineEngine::new(connections, network, settings)?))
    } else {
        let relay = config.relay.ok_or("relayまたはpipelineを指定してください")?;
        let connection = if registry.names().contains(&relay.plugin.as_str()) {
            registry.connect(&relay.plugin, context, relay.options).await?
        } else {
            store.connect(&relay.plugin, context, relay.options).await?
        };
        Runtime::Relay(Arc::new(Engine::new(connection, network, settings)?))
    };
    info!("中継を開始します: node={}, channel={}", config.node_id, config.channel);
    let shutdown = CancellationToken::new();
    let pipeline = match &engine {
        Runtime::Pipeline(engine) => Some(engine.clone()),
        Runtime::Relay(_) => None,
    };
    let reloader = amitoki::control::Reloader::new(amitoki::control::ReloadSource { path, baseline, store }, pipeline).with_runtime(engine.clone());
    let controlling = control_server.run(reloader, shutdown.clone());
    tokio::pin!(controlling);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let signals = async {
        tokio::select! { outcome = tokio::signal::ctrl_c() => outcome, _ = terminate.recv() => Ok(()) }
    };
    let running = engine.run(shutdown.clone());
    tokio::pin!(running);
    let outcome = tokio::select! {
        outcome = &mut running => outcome,
        control_outcome = &mut controlling => {
            shutdown.cancel();
            let _ = running.await;
            control_outcome?;
            return Err("制御処理が予期せず終了しました".into());
        },
        signal = signals => {
            shutdown.cancel();
            let outcome = running.await;
            signal?;
            outcome
        },
    };
    shutdown.cancel();
    info!(
        "中継終了: capture={}, publish={}, inject={}, filter={}, reject={}, retry={}",
        engine.metrics().captured.load(Ordering::Relaxed),
        engine.metrics().published.load(Ordering::Relaxed),
        engine.metrics().injected.load(Ordering::Relaxed),
        engine.metrics().filtered.load(Ordering::Relaxed),
        engine.metrics().rejected_deliveries.load(Ordering::Relaxed),
        engine.metrics().retries.load(Ordering::Relaxed)
    );
    engine.report_pipeline();
    outcome?;
    Ok(())
}
