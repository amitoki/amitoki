use crate::{
    wire::{read_message, write_message, Request, Response},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::RelayError;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

// 呼び出し側がタイムアウトしても、応答を読み終えてから次の要求を送る。
const REQUEST_QUEUE: usize = 8;
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
struct Call {
    request: Request,
    reply: oneshot::Sender<Result<Response, RelayError>>,
}
pub struct ProcessClient {
    sender: mpsc::Sender<Call>,
    worker: JoinHandle<()>,
}

impl Drop for ProcessClient {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

impl ProcessClient {
    pub(crate) async fn start(executable: &Path, manifest: &PluginManifest) -> Result<Self, RelayError> {
        manifest.validate()?;
        let mut command = Command::new(executable);
        command.arg("--stdio").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).kill_on_drop(true);
        crate::process_security::harden(&mut command);
        let child = command.spawn().map_err(|_| RelayError::permanent("プラグインを起動できません"))?;
        let (sender, receiver) = mpsc::channel(REQUEST_QUEUE);
        let worker = tokio::spawn(run(child, receiver));
        let process = Self { sender, worker };
        match process.call(Request::Describe).await? {
            Response::Manifest(actual)
                if actual.name == manifest.name
                    && actual.version == manifest.version
                    && actual.protocol_version == PROTOCOL_VERSION
                    && actual.config_schema == manifest.config_schema
                    && actual.block == manifest.block => {},
            _ => return Err(RelayError::permanent("実行ファイルとプラグインの定義が一致しません")),
        }
        Ok(process)
    }

    pub(crate) async fn call(&self, request: Request) -> Result<Response, RelayError> {
        let (reply, response) = oneshot::channel();
        self.sender.send(Call { request, reply }).await.map_err(|_| disconnected())?;
        match response.await.map_err(|_| disconnected())?? {
            Response::Error { message, retryable: true } => Err(RelayError::retryable(message)),
            Response::Error { message, retryable: false } => Err(RelayError::permanent(message)),
            response => Ok(response),
        }
    }

    pub(crate) async fn success(&self, request: Request) -> Result<(), RelayError> {
        match self.call(request).await? {
            Response::Success => Ok(()),
            _ => Err(RelayError::permanent("プラグインから予期しない応答が届きました")),
        }
    }
}

fn disconnected() -> RelayError {
    RelayError::permanent("プラグインとの通信が終了しました。中継を再起動してください")
}

async fn exchange(input: &mut ChildStdin, output: &mut ChildStdout, request: &Request) -> std::io::Result<Response> {
    write_message(input, request).await?;
    read_message(output).await
}

async fn run(mut child: Child, mut calls: mpsc::Receiver<Call>) {
    let mut input = child.stdin.take().expect("piped stdin");
    let mut output = child.stdout.take().expect("piped stdout");
    while let Some(call) = calls.recv().await {
        let response = tokio::time::timeout(IPC_TIMEOUT, exchange(&mut input, &mut output, &call.request)).await;
        match response {
            Ok(Ok(response)) => {
                let _ = call.reply.send(Ok(response));
            },
            _ => {
                let _ = call.reply.send(Err(disconnected()));
                break;
            },
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}
