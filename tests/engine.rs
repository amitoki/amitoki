use amitoki::{
    engine::{Engine, EngineConfig, EngineError, EngineSettings},
    firewall::{Firewall, Policy},
    network::PacketIo,
};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError, RelayPlugin};
use amitoki_relay_memory::MemoryPlugin;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_util::sync::CancellationToken;

fn frame_bytes(value: u8) -> Vec<u8> {
    let mut bytes = vec![value; 1514];
    bytes[12..14].copy_from_slice(&0x88b5_u16.to_be_bytes());
    bytes
}

struct TestNetwork {
    incoming: Mutex<mpsc::Receiver<Vec<u8>>>,
    outgoing: mpsc::Sender<Vec<u8>>,
    captured: Notify,
    send_count: AtomicUsize,
    fail_at: usize,
}

#[async_trait]
impl PacketIo for TestNetwork {
    async fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes = self.incoming.lock().await.recv().await.ok_or_else(|| io::Error::other("テスト入力が閉じました"))?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        self.captured.notify_one();
        Ok(bytes.len())
    }
    async fn send(&self, frame: &[u8]) -> io::Result<()> {
        if self.send_count.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at {
            return Err(io::Error::other("NIC送信の失敗を再現"));
        }
        self.outgoing.send(frame.to_vec()).await.map_err(|_| io::Error::other("テスト出力が閉じました"))
    }
}

struct FaultRelay {
    inner: Arc<dyn Relay>,
    uncertain_publish: AtomicBool,
    failed_ack: AtomicBool,
    block_publish: bool,
    published: Notify,
    acknowledged: Notify,
}

impl FaultRelay {
    fn new(inner: Arc<dyn Relay>) -> Self {
        Self {
            inner,
            uncertain_publish: AtomicBool::new(false),
            failed_ack: AtomicBool::new(false),
            block_publish: false,
            published: Notify::new(),
            acknowledged: Notify::new(),
        }
    }
}

#[async_trait]
impl Relay for FaultRelay {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        if self.block_publish {
            std::future::pending::<()>().await;
        }
        self.inner.publish(frames).await?;
        if self.uncertain_publish.swap(false, Ordering::SeqCst) {
            return Err(RelayError::retryable("受け付け後の通信断を再現"));
        }
        self.published.notify_one();
        Ok(())
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        self.inner.receive(limit).await
    }
    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        if self.failed_ack.swap(false, Ordering::SeqCst) {
            return Err(RelayError::retryable("ACK失敗を再現"));
        }
        self.inner.acknowledge(receipts).await?;
        self.acknowledged.notify_one();
        Ok(())
    }
}

struct RunningEngine {
    engine: Arc<Engine>,
    network: Arc<TestNetwork>,
    input: mpsc::Sender<Vec<u8>>,
    output: mpsc::Receiver<Vec<u8>>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<Result<(), EngineError>>,
}

fn start(relay: Arc<dyn Relay>, config: EngineConfig, fail_at: usize) -> RunningEngine {
    let (input, incoming) = mpsc::channel(32);
    let (outgoing, output) = mpsc::channel(32);
    let network = Arc::new(TestNetwork {
        incoming: Mutex::new(incoming),
        outgoing,
        captured: Notify::new(),
        send_count: AtomicUsize::new(0),
        fail_at,
    });
    let firewall = Firewall {
        policy: Policy::Blacklist,
        rules: vec![],
    };
    let engine = Arc::new(Engine::new(relay, network.clone(), EngineSettings { config, firewall }).unwrap());
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(engine.clone().run(shutdown.clone()));
    RunningEngine {
        engine,
        network,
        input,
        output,
        shutdown,
        task,
    }
}

async fn connect(plugin: &MemoryPlugin, node: &str) -> Arc<dyn Relay> {
    plugin
        .connect(
            RelayContext {
                node_id: node.into(),
                channel: "test".into(),
            },
            json!({}),
        )
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn retry_after_uncertain_publish_preserves_identity_and_delivers_one_frame() {
    let plugin = MemoryPlugin::default();
    let relay = Arc::new(FaultRelay::new(connect(&plugin, "a").await));
    relay.uncertain_publish.store(true, Ordering::SeqCst);
    let observer = connect(&plugin, "b").await;
    let run = start(
        relay.clone(),
        EngineConfig {
            batch_size: 1,
            ..Default::default()
        },
        usize::MAX,
    );
    run.input.send(frame_bytes(1)).await.unwrap();
    relay.published.notified().await;
    run.shutdown.cancel();
    run.task.await.unwrap().unwrap();
    assert_eq!(observer.receive(10).await.unwrap().len(), 1);
    assert_eq!(run.engine.metrics.published.load(Ordering::Relaxed), 1);
    assert_eq!(run.engine.metrics.retries.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn shutdown_flushes_a_partial_batch_without_waiting_for_the_flush_interval() {
    let plugin = MemoryPlugin::default();
    let source = connect(&plugin, "a").await;
    let observer = connect(&plugin, "b").await;
    let run = start(
        source,
        EngineConfig {
            flush_interval_ms: 60_000,
            ..Default::default()
        },
        usize::MAX,
    );
    run.input.send(frame_bytes(2)).await.unwrap();
    run.network.captured.notified().await;
    run.shutdown.cancel();
    run.task.await.unwrap().unwrap();
    assert_eq!(observer.receive(10).await.unwrap().len(), 1);
    assert_eq!(run.engine.metrics.pending_publish_count(), 0);
}

#[tokio::test(start_paused = true)]
async fn acknowledgement_retry_does_not_inject_a_frame_twice() {
    let plugin = MemoryPlugin::default();
    let source = connect(&plugin, "a").await;
    let destination = connect(&plugin, "b").await;
    let relay = Arc::new(FaultRelay::new(destination.clone()));
    relay.failed_ack.store(true, Ordering::SeqCst);
    source.publish(&[Frame::new(Bytes::from(frame_bytes(3))).unwrap()]).await.unwrap();
    let mut run = start(relay.clone(), EngineConfig::default(), usize::MAX);
    relay.acknowledged.notified().await;
    assert_eq!(run.output.recv().await.unwrap(), frame_bytes(3));
    assert!(run.output.try_recv().is_err());
    assert!(destination.receive(10).await.unwrap().is_empty());
    run.shutdown.cancel();
    run.task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn failed_network_send_acknowledges_only_the_successful_prefix() {
    let plugin = MemoryPlugin::default();
    let source = connect(&plugin, "a").await;
    let destination = connect(&plugin, "b").await;
    let frames: Vec<_> = (0..3).map(|value| Frame::new(Bytes::from(frame_bytes(value))).unwrap()).collect();
    source.publish(&frames).await.unwrap();
    let run = start(destination.clone(), EngineConfig::default(), 2);
    assert!(matches!(run.task.await.unwrap(), Err(EngineError::Network(_))));
    let pending = destination.receive(10).await.unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].frame.id, frames[1].id);
}

#[tokio::test(start_paused = true)]
async fn blocked_backend_bounds_capture_and_shutdown_reports_unsent_frames() {
    let plugin = MemoryPlugin::default();
    let mut relay = FaultRelay::new(connect(&plugin, "a").await);
    relay.block_publish = true;
    let run = start(
        Arc::new(relay),
        EngineConfig {
            batch_size: 1,
            queue_capacity: 1,
            shutdown_timeout_ms: 50,
            ..Default::default()
        },
        usize::MAX,
    );
    for value in 0..10 {
        run.input.send(frame_bytes(value)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(run.engine.metrics.captured.load(Ordering::Relaxed), 2);
    run.shutdown.cancel();
    assert!(matches!(run.task.await.unwrap(), Err(EngineError::ShutdownTimeout(2))));
}
