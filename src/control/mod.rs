//! nginx型reload。検証と初期化に成功した世代だけを公開する。
mod reload;
mod socket;
use crate::plugin_manager::{source::expand_path, ManagerResult};
use clap::Parser;
pub use reload::{ReloadSource, Reloader};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use tokio_util::sync::CancellationToken;

// 制御接続が要求を送らずに占有し続けることを防ぐ。
const CONTROL_IO_TIMEOUT: Duration = Duration::from_secs(5);
// 本体の最大reload_timeout（300秒）に応答転送の余裕を加える。
const RELOAD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(310);
const MAX_RESPONSE_BYTES: u64 = 8192;

#[derive(Serialize, Deserialize)]
struct Reply {
    generation: Option<u64>,
    error: Option<String>,
}

pub struct ControlServer {
    socket: socket::ControlSocket,
    hangup: tokio::signal::unix::Signal,
}

impl ControlServer {
    pub fn bind(path: &Path) -> ManagerResult<Self> {
        Ok(Self {
            socket: socket::ControlSocket::bind(path)?,
            hangup: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?,
        })
    }

    pub async fn run(mut self, reloader: Reloader, shutdown: CancellationToken) -> ManagerResult<()> {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                _ = self.hangup.recv() => {
                    let outcome = tokio::select! {
                        _ = shutdown.cancelled() => return Ok(()),
                        outcome = reloader.reload() => outcome,
                    };
                    report(outcome);
                },
                accepted = self.socket.listener.accept() => {
                    let (stream, _) = accepted?;
                    tokio::select! {
                        _ = shutdown.cancelled() => return Ok(()),
                        outcome = serve(stream, &reloader) => if let Err(error) = outcome { log::warn!("reload制御接続: {error}"); },
                    }
                },
            }
        }
    }
}

fn report(outcome: Result<u64, String>) -> Reply {
    match outcome {
        Ok(generation) => Reply {
            generation: Some(generation),
            error: None,
        },
        Err(error) => {
            log::warn!("reloadを拒否しました。旧Pipelineを継続します: {error}");
            Reply {
                generation: None,
                error: Some(error),
            }
        },
    }
}

async fn serve(mut stream: UnixStream, reloader: &Reloader) -> ManagerResult<()> {
    if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } {
        return Err("reloadは本体と同じユーザで実行してください".into());
    }
    let mut request = [0; 7];
    tokio::time::timeout(CONTROL_IO_TIMEOUT, stream.read_exact(&mut request)).await??;
    if &request != b"reload\n" {
        return Err("制御要求が不正です".into());
    }
    let reply = report(reloader.reload().await);
    let bytes = serde_json::to_vec(&reply)?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("reload応答が上限を超えました。ログを確認してください".into());
    }
    tokio::time::timeout(CONTROL_IO_TIMEOUT, stream.write_all(&bytes)).await??;
    Ok(())
}

#[derive(Parser)]
#[command(name = "amitoki reload", about = "検証済みのStage・経路へ切り替える。旧世代は配送完了まで保持する")]
struct Arguments {
    #[arg(long, default_value = "amitoki.toml")]
    config: PathBuf,
}

pub async fn run_cli(arguments: &[String]) -> ManagerResult<()> {
    let arguments = Arguments::parse_from(std::iter::once("amitoki reload".to_owned()).chain(arguments.iter().cloned()));
    let path = socket::path(&expand_path(&arguments.config)?)?;
    let request = async {
        let mut stream = UnixStream::connect(path).await?;
        stream.write_all(b"reload\n").await?;
        let mut bytes = Vec::new();
        stream.take(MAX_RESPONSE_BYTES + 1).read_to_end(&mut bytes).await?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("reload応答が上限を超えています".into());
        }
        let reply: Reply = serde_json::from_slice(&bytes)?;
        match (reply.generation, reply.error) {
            (Some(generation), None) => {
                println!("Pipelineを切り替えました: generation={generation}（旧世代は完了待ち）");
                Ok(())
            },
            (None, Some(error)) => Err(error.into()),
            _ => Err("reload応答が不正です".into()),
        }
    };
    tokio::time::timeout(RELOAD_RESPONSE_TIMEOUT, request).await?
}
