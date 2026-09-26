use crate::{
    wire::{read_message, write_message, Request, Response, MAX_BATCH},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError};
use async_trait::async_trait;
use serde_json::Value;
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
pub struct ProcessRelay {
    sender: mpsc::Sender<Call>,
    worker: JoinHandle<()>,
}

impl Drop for ProcessRelay {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

impl ProcessRelay {
    pub async fn connect(executable: &Path, manifest: &PluginManifest, configuration: (RelayContext, Value)) -> Result<Self, RelayError> {
        let (context, options) = configuration;
        manifest.validate_options(&options)?;
        let mut command = Command::new(executable);
        command.arg("--stdio").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).kill_on_drop(true);
        let child = command.spawn().map_err(|_| RelayError::permanent("プラグインを起動できません"))?;
        let (sender, receiver) = mpsc::channel(REQUEST_QUEUE);
        let worker = tokio::spawn(run(child, receiver));
        let relay = Self { sender, worker };
        match relay.call(Request::Describe).await? {
            Response::Manifest(actual)
                if actual.name == manifest.name
                    && actual.version == manifest.version
                    && actual.protocol_version == PROTOCOL_VERSION
                    && actual.config_schema == manifest.config_schema => {},
            _ => return Err(RelayError::permanent("実行ファイルとプラグインの定義が一致しません")),
        }
        relay
            .success(Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                context,
                options,
            })
            .await?;
        Ok(relay)
    }

    async fn call(&self, request: Request) -> Result<Response, RelayError> {
        let (reply, response) = oneshot::channel();
        self.sender.send(Call { request, reply }).await.map_err(|_| disconnected())?;
        match response.await.map_err(|_| disconnected())?? {
            Response::Error { message, retryable: true } => Err(RelayError::retryable(message)),
            Response::Error { message, retryable: false } => Err(RelayError::permanent(message)),
            response => Ok(response),
        }
    }

    async fn success(&self, request: Request) -> Result<(), RelayError> {
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

#[async_trait]
impl Relay for ProcessRelay {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        for frame in frames {
            frame.validate()?;
        }
        for batch in frames.chunks(MAX_BATCH) {
            self.success(Request::Publish { frames: batch.to_vec() }).await?;
        }
        Ok(())
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        let limit = limit.min(MAX_BATCH);
        match self.call(Request::Receive { limit }).await? {
            Response::Deliveries(deliveries) if deliveries.len() <= limit => {
                for delivery in &deliveries {
                    delivery.frame.validate()?;
                }
                Ok(deliveries)
            },
            _ => Err(RelayError::permanent("プラグインの受信応答が不正です")),
        }
    }
    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        for batch in receipts.chunks(MAX_BATCH) {
            self.success(Request::Acknowledge { receipts: batch.to_vec() }).await?;
        }
        Ok(())
    }
}
