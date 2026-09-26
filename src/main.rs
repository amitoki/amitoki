use amitoki::{
    config::AppConfig,
    engine::{Engine, EngineSettings},
    network::LinuxSocket,
    plugin_manager::{run_cli, PluginStore},
    plugins::builtin_plugins,
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
    if arguments.first().map(String::as_str) == Some("plugin") {
        return run_cli(&arguments[1..]).await;
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
        println!("amitoki [--config PATH] [--check-config]\namitoki --list-plugins\namitoki plugin --help");
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
    let config = AppConfig::load(&path)?;
    if !registry.names().contains(&config.relay.plugin.as_str()) {
        store.resolved_options(&config.relay.plugin, &config.relay.options)?;
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
    let network = Arc::new(LinuxSocket::open(&config.interface, config.promiscuous)?);
    let relay = if registry.names().contains(&config.relay.plugin.as_str()) {
        registry.connect(&config.relay.plugin, config.context(), config.relay.options).await?
    } else {
        store.connect(&config.relay.plugin, config.context(), config.relay.options).await?
    };
    let engine = Arc::new(Engine::new(
        relay,
        network,
        EngineSettings {
            firewall: config.firewall,
            config: config.engine,
        },
    )?);
    info!("中継を開始します: node={}, channel={}, plugin={}", config.node_id, config.channel, config.relay.plugin);
    let shutdown = CancellationToken::new();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let signals = async {
        tokio::select! { outcome = tokio::signal::ctrl_c() => outcome, _ = terminate.recv() => Ok(()) }
    };
    let running = engine.clone().run(shutdown.clone());
    tokio::pin!(running);
    let outcome = tokio::select! {
        outcome = &mut running => outcome,
        signal = signals => {
            shutdown.cancel();
            let outcome = running.await;
            signal?;
            outcome
        },
    };
    info!(
        "中継終了: capture={}, publish={}, inject={}, filter={}, reject={}, retry={}",
        engine.metrics.captured.load(Ordering::Relaxed),
        engine.metrics.published.load(Ordering::Relaxed),
        engine.metrics.injected.load(Ordering::Relaxed),
        engine.metrics.filtered.load(Ordering::Relaxed),
        engine.metrics.rejected_deliveries.load(Ordering::Relaxed),
        engine.metrics.retries.load(Ordering::Relaxed)
    );
    outcome?;
    Ok(())
}
