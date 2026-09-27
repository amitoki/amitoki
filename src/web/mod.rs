//! ローカルの解析・監視UI。転送本体とは別プロセスで動く。
mod assets;
mod replay;
mod server;
use crate::{
    config::AppConfig,
    observation::Topology,
    plugin_manager::{expand_path, ManagerResult, PluginStore},
};
use clap::Parser;
use std::{
    io::{Read, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::sync::Semaphore;

// 対話的な解析に限定し、HTTP入力・解析結果・子プロセスの保持量を制限する。
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_PACKETS: u64 = 1000;
const REPLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Parser)]
#[command(name = "amitoki web", about = "PCAP解析・パイプライン・本体の稼働状況をローカルのブラウザで表示する")]
struct Arguments {
    #[arg(long, default_value = "amitoki.toml")]
    config: PathBuf,
    #[arg(long)]
    directory: Option<PathBuf>,
    #[arg(long)]
    pcap: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:8710")]
    listen: SocketAddr,
}

struct State {
    topology: Topology,
    config: PathBuf,
    snapshot: tempfile::NamedTempFile,
    plugins: PathBuf,
    executable: PathBuf,
    capture: RwLock<bytes::Bytes>,
    replay_slot: Semaphore,
    authority: String,
    token: String,
}

pub async fn run_cli(arguments: &[String]) -> ManagerResult<()> {
    let arguments = Arguments::parse_from(std::iter::once("amitoki web".to_owned()).chain(arguments.iter().cloned()));
    if !arguments.listen.ip().is_loopback() {
        return Err("--listenにはループバックアドレスを指定してください".into());
    }
    let config = expand_path(&arguments.config)?.canonicalize()?;
    let (snapshot, loaded) = freeze_config(&config)?;
    let plugins = match arguments.directory {
        Some(directory) => expand_path(&directory)?,
        None => PluginStore::from_environment()?.directory,
    };
    let plugins = if plugins.is_absolute() { plugins } else { std::env::current_dir()?.join(plugins) };
    let listener = tokio::net::TcpListener::bind(arguments.listen).await?;
    let authority = listener.local_addr()?.to_string();
    let state = Arc::new(State {
        topology: Topology::new(&loaded),
        config,
        snapshot,
        plugins,
        executable: std::env::current_exe()?,
        capture: RwLock::new(bytes::Bytes::from_static(b"{\"name\":null,\"source\":\"capture\",\"packets\":[],\"truncated\":false}")),
        replay_slot: Semaphore::new(1),
        authority,
        token: uuid::Uuid::new_v4().to_string(),
    });
    if let Some(path) = arguments.pcap {
        let path = expand_path(&path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?.take(MAX_CAPTURE_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CAPTURE_BYTES {
            return Err("PCAPは16MiB以内にしてください".into());
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let capture = replay::analyze(
            &state,
            replay::CaptureInput {
                bytes: &bytes,
                name: &name,
                source: "capture",
            },
        )
        .await?;
        *state.capture.write().map_err(|_| "解析結果を保存できません")? = capture;
    }
    println!("http://{}/#{}", state.authority, state.token);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(async move {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        })
        .await?;
    Ok(())
}

fn freeze_config(path: &std::path::Path) -> ManagerResult<(tempfile::NamedTempFile, AppConfig)> {
    // UIの再生は起動時の構成で固定する。稼働中の世代はstatus APIで別に取得する。
    const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
    let mut text = String::new();
    std::fs::File::open(path)?.take(MAX_CONFIG_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err("設定は1MiB以内にしてください".into());
    }
    let mut document: toml::Value = toml::from_str(&text).map_err(|_| "設定のTOMLが不正です")?;
    if let Some(file) = document.get_mut("pipeline_file") {
        let expanded = expand_path(&PathBuf::from(file.as_str().ok_or("pipeline_fileが不正です")?))?;
        let absolute = if expanded.is_absolute() { expanded } else { path.parent().unwrap().join(expanded) };
        *file = toml::Value::String(absolute.to_str().ok_or("PipelineのパスがUTF-8ではありません")?.to_owned());
    }
    let mut snapshot = tempfile::NamedTempFile::new()?;
    snapshot.write_all(toml::to_string(&document)?.as_bytes())?;
    let loaded = AppConfig::load(snapshot.path())?;
    if let Some(pipeline) = &loaded.pipeline {
        let table = document.as_table_mut().ok_or("設定が不正です")?;
        table.remove("pipeline_file");
        table.insert("pipeline".into(), toml::Value::try_from(pipeline)?);
        snapshot.as_file_mut().set_len(0)?;
        use std::io::Seek;
        snapshot.as_file_mut().rewind()?;
        snapshot.write_all(toml::to_string(&document)?.as_bytes())?;
    }
    Ok((snapshot, loaded))
}
