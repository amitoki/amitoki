use super::*;
use crate::{
    firewall::{Filter, Firewall, Policy},
    packet::MacAddress,
    pipeline::{graph::Graph, plan::Planner, ErrorPolicy, PipelineConfig, PipelineMetrics, RunningBlock},
};
use amitoki_plugin_sdk::block::{Block, BlockDefinition, BlockOutput, BlockPacket};
use amitoki_relay::RelayError;
use async_trait::async_trait;
use serde_json::json;
use std::{sync::Arc, time::Duration};

struct Rewrite(u8);
#[async_trait]
impl Block for Rewrite {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        Ok(packets
            .iter()
            .map(|packet| {
                let mut bytes = packet.frame.bytes.to_vec();
                bytes[0] = self.0;
                BlockOutput {
                    ports: vec!["out".into()],
                    annotations: packet.annotations.clone(),
                    bytes: Some(bytes.into()),
                }
            })
            .collect())
    }
}
fn original() -> Frame {
    let mut bytes = vec![1; 60];
    bytes[12..14].copy_from_slice(&0x88b5u16.to_be_bytes());
    Frame::new(bytes.into()).unwrap()
}
fn config(merge: bool) -> PipelineConfig {
    serde_json::from_value(json!({
        "relays":[{"id":"output","plugin":"memory"}],
        "stages":[{"id":"rewrite","plugin":"custom"}],
        "routes":[{"from":"capture","to":if merge {vec!["rewrite","output"]} else {vec!["rewrite"]}},
            {"from":"rewrite.out","to":["output"]},{"from":"output.received","to":[]}]
    }))
    .unwrap()
}
fn block(rewrite: bool) -> Vec<RunningBlock> {
    vec![RunningBlock {
        block: Arc::new(Rewrite(2)),
        definition: BlockDefinition {
            outputs: vec!["out".into()],
            rewrite,
        },
        on_error: ErrorPolicy::Stop,
    }]
}

#[test]
fn rewritten_identity_is_stable_per_original_and_content_and_restores_on_no_change() {
    let frame = original();
    let mut bytes = frame.bytes.to_vec();
    bytes[0] = 2;
    let first = rewritten_frame(&frame, bytes.clone().into());
    assert_ne!(first.id, frame.id);
    assert_eq!(first, rewritten_frame(&frame, bytes.clone().into()));
    assert_ne!(first.id, rewritten_frame(&original(), bytes.clone().into()).id);
    bytes[0] = 3;
    assert_ne!(first.id, rewritten_frame(&frame, bytes.into()).id);
    assert_eq!(rewritten_frame(&frame, frame.bytes.clone()), frame);
}

#[tokio::test]
async fn terminal_receives_rewritten_bytes_and_core_assigned_identity() {
    let blocks = block(true);
    let graph = Graph::compile(&config(false), &[blocks[0].definition.clone()]).unwrap();
    let metrics = PipelineMetrics::default();
    let firewall = Firewall {
        policy: Policy::Blacklist,
        rules: vec![],
    };
    let planner = Planner {
        graph: &graph,
        blocks: &blocks,
        metrics: &metrics,
        firewall: &firewall,
        operation_timeout: Duration::from_secs(1),
    };
    let frame = original();
    let plan = planner.prepare_traced(std::slice::from_ref(&frame), &graph.capture, None).await.unwrap();
    assert_eq!(plan.relays[0][0].bytes[0], 2);
    assert_ne!(plan.relays[0][0].id, frame.id);
    assert_eq!(frame.bytes[0], 1);
}

#[tokio::test]
async fn undeclared_rewrites_firewall_bypass_and_conflicting_merges_are_rejected() {
    let blocked = Firewall {
        policy: Policy::Blacklist,
        rules: vec![Filter::DstMacAddress(MacAddress([2, 1, 1, 1, 1, 1]))],
    };
    let allowed = Firewall {
        policy: Policy::Blacklist,
        rules: vec![],
    };
    for (rewrite, merge, firewall) in [(false, false, &allowed), (true, false, &blocked), (true, true, &allowed)] {
        let blocks = block(rewrite);
        let graph = Graph::compile(&config(merge), &[blocks[0].definition.clone()]).unwrap();
        let metrics = PipelineMetrics::default();
        let planner = Planner {
            graph: &graph,
            blocks: &blocks,
            metrics: &metrics,
            firewall,
            operation_timeout: Duration::from_secs(1),
        };
        assert!(planner.prepare_traced(&[original()], &graph.capture, None).await.is_err());
    }
}
