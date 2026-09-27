use amitoki::{
    engine::{EngineConfig, EngineSettings},
    firewall::{Firewall, Policy},
    network::PacketIo,
    pipeline::{graph::Graph, Connections, ErrorPolicy, PipelineConfig, PipelineEngine, RunningBlock},
};
use amitoki_plugin_sdk::block::{Block, BlockDefinition, BlockOutput, BlockPacket};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayError};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;
use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Transport {
    published: Mutex<Vec<Frame>>,
    incoming: Mutex<VecDeque<Delivery>>,
    acknowledgements: Mutex<Vec<Receipt>>,
    publish_calls: AtomicUsize,
    ack_calls: AtomicUsize,
    failures: usize,
    publish_gate: Option<Arc<AcknowledgementGate>>,
    permanent_publish_failure: bool,
    permanent_receive_failure: bool,
    ack_failures: usize,
    ack_gate: Option<Arc<AcknowledgementGate>>,
    complete: Notify,
}
#[derive(Default)]
struct AcknowledgementGate {
    entered: Notify,
    released: Notify,
}
#[async_trait]
impl Relay for Transport {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        if self.permanent_publish_failure {
            return Err(RelayError::permanent("送信プロセス終了"));
        }
        if let Some(gate) = &self.publish_gate {
            gate.entered.notify_one();
            gate.released.notified().await;
        }
        if self.publish_calls.fetch_add(1, Ordering::SeqCst) < self.failures {
            return Err(RelayError::retryable("送信障害"));
        }
        self.published.lock().unwrap().extend_from_slice(frames);
        self.complete.notify_one();
        Ok(())
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        if self.permanent_receive_failure {
            return Err(RelayError::permanent("受信プロセス終了"));
        }
        Ok(self.incoming.lock().unwrap().iter().take(limit).cloned().collect())
    }
    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        let attempt = self.ack_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.ack_gate {
            if attempt == 0 {
                gate.entered.notify_one();
                gate.released.notified().await;
            }
        }
        if attempt < self.ack_failures {
            return Err(RelayError::retryable("ACK障害"));
        }
        self.incoming.lock().unwrap().retain(|delivery| !receipts.contains(&delivery.receipt));
        self.acknowledgements.lock().unwrap().extend_from_slice(receipts);
        self.complete.notify_one();
        Ok(())
    }
}
struct Network {
    input: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
    output: Mutex<Vec<Vec<u8>>>,
    fail_at: AtomicUsize,
    received: Notify,
}
#[async_trait]
impl PacketIo for Network {
    async fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes = self.input.lock().await.recv().await.ok_or_else(|| io::Error::other("入力終了"))?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        self.received.notify_one();
        Ok(bytes.len())
    }
    async fn send(&self, bytes: &[u8]) -> io::Result<()> {
        let mut output = self.output.lock().unwrap();
        if output.len() + 1 == self.fail_at.load(Ordering::SeqCst) {
            return Err(io::Error::other("NIC障害"));
        }
        output.push(bytes.to_vec());
        Ok(())
    }
}
struct Classifier {
    calls: AtomicUsize,
    fail: bool,
    hang: bool,
    invalid_port: bool,
}
impl Classifier {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail: false,
            hang: false,
            invalid_port: false,
        }
    }
}
#[async_trait]
impl Block for Classifier {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        self.calls.fetch_add(packets.len(), Ordering::SeqCst);
        if self.hang {
            return std::future::pending().await;
        }
        if self.fail {
            return Err(RelayError::retryable("解析失敗"));
        }
        Ok(packets
            .iter()
            .map(|packet| BlockOutput {
                bytes: None,
                ports: vec![if self.invalid_port {
                    "inject"
                } else if packet.frame.bytes[0] == 9 {
                    "drop"
                } else {
                    "pass"
                }
                .into()],
                annotations: json!({"checked":true}),
            })
            .collect())
    }
}
fn bytes(value: u8) -> Vec<u8> {
    let mut bytes = vec![value; 64];
    bytes[12..14].copy_from_slice(&0x88b5u16.to_be_bytes());
    bytes
}
fn frame(value: u8) -> Frame {
    Frame::new(Bytes::from(bytes(value))).unwrap()
}
fn enqueue(transport: &Transport, frames: &[Frame]) {
    transport.incoming.lock().unwrap().extend(frames.iter().map(|frame| Delivery {
        frame: frame.clone(),
        receipt: Receipt(frame.id.to_string()),
    }));
}
fn configuration() -> PipelineConfig {
    serde_json::from_value(
        json!({"relays":[{"id":"first","plugin":"fixture"},{"id":"second","plugin":"fixture"}],"blocks":[{"id":"classify","plugin":"fixture"}],"routes":[
            {"from":"capture","to":["classify"]},{"from":"classify.pass","to":["first","second"]},{"from":"classify.drop","to":[]},
            {"from":"first.received","to":["inject"]},{"from":"second.received","to":["inject"]}
        ]}),
    )
    .unwrap()
}
struct Harness {
    engine: Arc<PipelineEngine>,
    network: Arc<Network>,
    input: mpsc::Sender<Vec<u8>>,
    shutdown: CancellationToken,
}
fn harness(transports: [Arc<Transport>; 2], block: Arc<dyn Block>, config: PipelineConfig) -> Harness {
    harness_with_settings(
        transports,
        block,
        (
            config,
            EngineConfig {
                batch_size: 1,
                ..Default::default()
            },
        ),
    )
}
fn harness_with_settings(transports: [Arc<Transport>; 2], block: Arc<dyn Block>, configuration: (PipelineConfig, EngineConfig)) -> Harness {
    let (config, settings) = configuration;
    let definition = BlockDefinition {
        rewrite: config.blocks[0].options["rewrite"].as_bool().unwrap_or(false),
        outputs: vec!["pass".into(), "drop".into()],
    };
    let graph = Graph::compile(&config, std::slice::from_ref(&definition)).unwrap();
    let (input, incoming) = mpsc::channel(16);
    let network = Arc::new(Network {
        input: tokio::sync::Mutex::new(incoming),
        output: Mutex::new(vec![]),
        fail_at: AtomicUsize::new(usize::MAX),
        received: Notify::new(),
    });
    let connections = Connections {
        graph,
        blocks: vec![RunningBlock {
            block,
            definition,
            on_error: config.blocks[0].on_error,
        }],
        relays: transports.into_iter().map(|relay| relay as Arc<dyn Relay>).collect(),
    };
    let settings = EngineSettings {
        config: settings,
        firewall: Firewall {
            policy: Policy::Blacklist,
            rules: vec![],
        },
    };
    let engine = Arc::new(PipelineEngine::new(connections, network.clone(), settings).unwrap());
    Harness {
        engine,
        network,
        input,
        shutdown: CancellationToken::new(),
    }
}
#[tokio::test(start_paused = true)]
async fn a_failed_branch_retries_without_repeating_analysis_or_successful_branches() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport {
        failures: 2,
        ..Default::default()
    });
    let block = Arc::new(Classifier::new());
    let run = harness([first.clone(), second.clone()], block.clone(), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(first.publish_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second.publish_calls.load(Ordering::SeqCst), 3);
    assert_eq!(block.calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.published.lock().unwrap()[0].id, second.published.lock().unwrap()[0].id);
}
#[tokio::test(start_paused = true)]
async fn duplicate_deliveries_from_two_relays_are_injected_once_and_acknowledged_to_each_source() {
    let first = Arc::new(Transport {
        ack_failures: 1,
        ..Default::default()
    });
    let second = Arc::new(Transport::default());
    let packet = frame(2);
    enqueue(&first, std::slice::from_ref(&packet));
    enqueue(&second, std::slice::from_ref(&packet));
    let run = harness([first.clone(), second.clone()], Arc::new(Classifier::new()), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(first.acknowledgements.lock().unwrap().len(), 1);
    assert_eq!(second.acknowledgements.lock().unwrap().len(), 1);
    assert_eq!(first.ack_calls.load(Ordering::SeqCst), 2);
}
#[tokio::test(start_paused = true)]
async fn a_custom_filter_drops_packets_without_publishing_them() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let block = Arc::new(Classifier::new());
    let run = harness([first.clone(), second.clone()], block.clone(), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(9)).await.unwrap();
    run.network.received.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(block.calls.load(Ordering::SeqCst), 1);
    assert!(first.published.lock().unwrap().is_empty());
    assert!(second.published.lock().unwrap().is_empty());
}
#[tokio::test(start_paused = true)]
async fn optional_analysis_failure_drops_its_branch_and_preserves_other_branches() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let block = Arc::new(Classifier { fail: true, ..Classifier::new() });
    let mut config = configuration();
    config.routes[0].to.push("first".into());
    config.blocks[0].on_error = ErrorPolicy::DropBranch;
    let run = harness([first.clone(), second.clone()], block, config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    first.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(first.published.lock().unwrap().len(), 1);
    assert!(second.published.lock().unwrap().is_empty());
    assert_eq!(run.engine.pipeline_metrics.dropped_branches.load(Ordering::Relaxed), 1);
}
#[tokio::test(start_paused = true)]
async fn invalid_output_cannot_select_a_core_endpoint_and_stops_delivery() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let run = harness(
        [first.clone(), second.clone()],
        Arc::new(Classifier {
            invalid_port: true,
            ..Classifier::new()
        }),
        configuration(),
    );
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert!(first.published.lock().unwrap().is_empty());
    assert!(run.network.output.lock().unwrap().is_empty());
}
#[tokio::test(start_paused = true)]
async fn an_unresponsive_block_times_out_without_publishing_or_retrying_analysis() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let block = Arc::new(Classifier { hang: true, ..Classifier::new() });
    let run = harness([first.clone(), second], block.clone(), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(block.calls.load(Ordering::SeqCst), 1);
    assert!(first.published.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_nic_send_leaves_that_delivery_and_its_successors_unacknowledged() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let frames: Vec<_> = (1..=3).map(frame).collect();
    enqueue(&first, &frames);
    let run = harness([first.clone(), second], Arc::new(Classifier::new()), configuration());
    run.network.fail_at.store(2, Ordering::SeqCst);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    assert!(task.await.unwrap().is_err());
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(first.acknowledgements.lock().unwrap().len(), 1);
    assert_eq!(first.incoming.lock().unwrap()[0].frame.id, frames[1].id);
    assert_eq!(first.incoming.lock().unwrap().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_receive_filter_acknowledges_rejected_packets_without_injecting_them() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    enqueue(&first, &[frame(9)]);
    let mut config = configuration();
    config.routes[0].to = vec!["first".into()];
    config.routes[1].to = vec!["inject".into()];
    config.routes[3].to = vec!["classify".into()];
    let block = Arc::new(Classifier::new());
    let run = harness([first.clone(), second], block.clone(), config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(block.calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.acknowledgements.lock().unwrap().len(), 1);
    assert!(run.network.output.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn discarding_one_received_route_does_not_suppress_injection_from_another_route() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let packet = frame(4);
    enqueue(&first, std::slice::from_ref(&packet));
    let mut config = configuration();
    config.routes[3].to.clear();
    let run = harness([first.clone(), second.clone()], Arc::new(Classifier::new()), config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    assert!(run.network.output.lock().unwrap().is_empty());
    enqueue(&second, std::slice::from_ref(&packet));
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(run.network.output.lock().unwrap()[0], packet.bytes);
}

#[tokio::test(start_paused = true)]
async fn each_received_route_runs_its_analysis_while_nic_injection_is_deduplicated() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let packet = frame(4);
    enqueue(&first, std::slice::from_ref(&packet));
    enqueue(&second, std::slice::from_ref(&packet));
    let mut config = configuration();
    config.routes[0].to = vec!["first".into()];
    config.routes[1].to = vec!["inject".into()];
    config.routes[3].to = vec!["classify".into()];
    config.routes[4].to = vec!["classify".into()];
    let block = Arc::new(Classifier::new());
    let run = harness([first.clone(), second.clone()], block.clone(), config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(block.calls.load(Ordering::SeqCst), 2);
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
}

struct WaitingStage {
    entered: Notify,
    released: Notify,
}
#[async_trait]
impl Block for WaitingStage {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        self.entered.notify_one();
        self.released.notified().await;
        Ok(packets
            .iter()
            .map(|packet| BlockOutput {
                bytes: None,
                ports: vec!["pass".into()],
                annotations: packet.annotations.clone(),
            })
            .collect())
    }
}

fn replacement(config: &PipelineConfig, block: Arc<dyn Block>) -> amitoki::pipeline::PreparedPipeline {
    amitoki::pipeline::PreparedPipeline::new(
        config,
        vec![RunningBlock {
            block,
            definition: BlockDefinition {
                rewrite: false,
                outputs: vec!["pass".into(), "drop".into()],
            },
            on_error: ErrorPolicy::Stop,
        }],
    )
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn reload_keeps_an_inflight_packet_on_the_old_route_and_sends_new_packets_to_the_new_route() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let old_stage = Arc::new(WaitingStage {
        entered: Notify::new(),
        released: Notify::new(),
    });
    let mut old_config = configuration();
    old_config.routes[1].to = vec!["first".into()];
    let run = harness([first.clone(), second.clone()], old_stage.clone(), old_config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    old_stage.entered.notified().await;
    let mut next_config = configuration();
    next_config.routes[1].to = vec!["second".into()];
    let next_stage = Arc::new(Classifier::new());
    assert_eq!(run.engine.activate(replacement(&next_config, next_stage.clone())).unwrap(), 2);
    run.input.send(bytes(2)).await.unwrap();
    old_stage.released.notify_one();
    first.complete.notified().await;
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(first.published.lock().unwrap().iter().map(|frame| frame.bytes[0]).collect::<Vec<_>>(), vec![1]);
    assert_eq!(second.published.lock().unwrap().iter().map(|frame| frame.bytes[0]).collect::<Vec<_>>(), vec![2]);
    assert_eq!(next_stage.calls.load(Ordering::SeqCst), 1);
    assert_eq!(Arc::strong_count(&old_stage), 1);
}

#[tokio::test(start_paused = true)]
async fn reloading_preserves_completed_receipts_and_global_nic_deduplication() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport::default());
    let packet = frame(4);
    enqueue(&first, std::slice::from_ref(&packet));
    let config = configuration();
    let run = harness([first.clone(), second.clone()], Arc::new(Classifier::new()), config.clone());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    run.engine.activate(replacement(&config, Arc::new(Classifier::new()))).unwrap();
    enqueue(&second, std::slice::from_ref(&packet));
    second.complete.notified().await;
    enqueue(&first, std::slice::from_ref(&packet));
    first.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(first.acknowledgements.lock().unwrap().len(), 2);
    assert_eq!(second.acknowledgements.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn reload_retains_old_stages_until_ack_retry_finishes_without_repeating_analysis_or_injection() {
    let gate = Arc::new(AcknowledgementGate::default());
    let first = Arc::new(Transport {
        ack_failures: 1,
        ack_gate: Some(gate.clone()),
        ..Default::default()
    });
    let second = Arc::new(Transport::default());
    let packet = frame(3);
    enqueue(&first, std::slice::from_ref(&packet));
    let mut config = configuration();
    config.routes[0].to = vec!["first".into()];
    config.routes[1].to = vec!["inject".into()];
    config.routes[3].to = vec!["classify".into()];
    let old_stage = Arc::new(Classifier::new());
    let next_stage = Arc::new(Classifier::new());
    let run = harness([first.clone(), second], old_stage.clone(), config.clone());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    gate.entered.notified().await;
    run.engine.activate(replacement(&config, next_stage.clone())).unwrap();
    assert!(Arc::strong_count(&old_stage) > 1, "ACK待ちの旧Stageが破棄された");
    gate.released.notify_one();
    first.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(Arc::strong_count(&old_stage), 1);
    assert_eq!(old_stage.calls.load(Ordering::SeqCst), 1);
    assert_eq!(next_stage.calls.load(Ordering::SeqCst), 0);
    assert_eq!(first.ack_calls.load(Ordering::SeqCst), 2);
    assert_eq!(first.acknowledgements.lock().unwrap().len(), 1);
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_full_failed_relay_queue_does_not_block_later_packets_to_a_healthy_relay() {
    let first = Arc::new(Transport::default());
    let gate = Arc::new(AcknowledgementGate::default());
    let second = Arc::new(Transport {
        publish_gate: Some(gate.clone()),
        ..Default::default()
    });
    let settings = EngineConfig {
        batch_size: 1,
        relay_queue_capacity: 1,
        ..Default::default()
    };
    let run = harness_with_settings([first.clone(), second.clone()], Arc::new(Classifier::new()), (configuration(), settings));
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    gate.entered.notified().await;
    first.complete.notified().await;
    for value in 2..=4 {
        run.input.send(bytes(value)).await.unwrap();
        first.complete.notified().await;
    }
    assert_eq!(first.published.lock().unwrap().len(), 4);
    assert_eq!(run.engine.relay_metrics[1].dropped.load(Ordering::Relaxed), 3);
    assert_eq!(run.engine.relay_metrics[1].queued.load(Ordering::Relaxed), 1);
    assert_eq!(run.engine.relay_metrics[1].peak_queued.load(Ordering::Relaxed), 1);
    gate.released.notify_one();
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(second.published.lock().unwrap()[0].bytes[0], 1);
    assert_eq!(run.engine.relay_metrics[1].queued.load(Ordering::Relaxed), 0);
}

#[tokio::test(start_paused = true)]
async fn a_blocked_ack_does_not_block_another_receiver_or_capture() {
    let gate = Arc::new(AcknowledgementGate::default());
    let first = Arc::new(Transport {
        ack_gate: Some(gate.clone()),
        ..Default::default()
    });
    let second = Arc::new(Transport::default());
    enqueue(&first, &[frame(1)]);
    let run = harness([first.clone(), second.clone()], Arc::new(Classifier::new()), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    gate.entered.notified().await;
    enqueue(&second, &[frame(2)]);
    second.complete.notified().await;
    assert_eq!(run.network.output.lock().unwrap().len(), 2);
    run.input.send(bytes(3)).await.unwrap();
    second.complete.notified().await;
    assert_eq!(second.published.lock().unwrap().len(), 1);
    gate.released.notify_one();
    first.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_crashed_relay_does_not_stop_capture_or_other_relays() {
    let first = Arc::new(Transport {
        permanent_publish_failure: true,
        permanent_receive_failure: true,
        ..Default::default()
    });
    let second = Arc::new(Transport::default());
    let run = harness([first, second.clone()], Arc::new(Classifier::new()), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    for value in 1..=4 {
        run.input.send(bytes(value)).await.unwrap();
        second.complete.notified().await;
    }
    enqueue(&second, &[frame(5)]);
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(second.published.lock().unwrap().len(), 4);
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(run.engine.relay_metrics[0].dropped.load(Ordering::Relaxed), 4);
    assert_eq!(run.engine.relay_metrics[0].failures.load(Ordering::Relaxed), 2);
}

#[tokio::test(start_paused = true)]
async fn shutdown_reports_undelivered_branch_frames_and_releases_queued_generations() {
    let first = Arc::new(Transport::default());
    let second = Arc::new(Transport {
        failures: usize::MAX,
        ..Default::default()
    });
    let run = harness([first.clone(), second], Arc::new(Classifier::new()), configuration());
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    run.input.send(bytes(1)).await.unwrap();
    first.complete.notified().await;
    run.shutdown.cancel();
    assert!(matches!(task.await.unwrap(), Err(amitoki::engine::EngineError::ShutdownTimeout(1))));
    assert_eq!(run.engine.relay_metrics[1].queued.load(Ordering::Relaxed), 0);
}

struct RewriteStage;
#[async_trait]
impl Block for RewriteStage {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        Ok(packets
            .iter()
            .map(|packet| {
                let mut bytes = packet.frame.bytes.to_vec();
                bytes[0] = 7;
                BlockOutput {
                    bytes: Some(bytes.into()),
                    ports: vec!["pass".into()],
                    annotations: packet.annotations.clone(),
                }
            })
            .collect())
    }
}

#[tokio::test(start_paused = true)]
async fn received_rewrites_inject_changed_bytes_once_and_acknowledge_original_receipts() {
    let first = Arc::new(Transport {
        ack_failures: 1,
        ..Default::default()
    });
    let second = Arc::new(Transport::default());
    let packet = frame(4);
    enqueue(&first, std::slice::from_ref(&packet));
    enqueue(&second, std::slice::from_ref(&packet));
    let mut config = configuration();
    config.blocks[0].options = json!({"rewrite":true});
    config.routes[0].to = vec!["first".into()];
    config.routes[1].to = vec!["inject".into()];
    config.routes[3].to = vec!["classify".into()];
    config.routes[4].to = vec!["classify".into()];
    let run = harness([first.clone(), second.clone()], Arc::new(RewriteStage), config);
    let task = tokio::spawn(run.engine.clone().run(run.shutdown.clone()));
    first.complete.notified().await;
    second.complete.notified().await;
    run.shutdown.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(run.network.output.lock().unwrap().len(), 1);
    assert_eq!(run.network.output.lock().unwrap()[0][0], 7);
    for relay in [first, second] {
        assert_eq!(*relay.acknowledgements.lock().unwrap(), vec![Receipt(packet.id.to_string())]);
    }
}
