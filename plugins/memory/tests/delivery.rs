use amitoki_relay::{Frame, PluginRegistry, RelayContext};
use amitoki_relay_memory::MemoryPlugin;
use bytes::Bytes;
use serde_json::json;

fn context(channel: &str, node: &str) -> RelayContext {
    RelayContext {
        node_id: node.into(),
        channel: channel.into(),
    }
}

#[tokio::test]
async fn each_node_receives_once_after_ack_and_publish_retries_do_not_duplicate() {
    let mut registry = PluginRegistry::default();
    registry.register(MemoryPlugin::default()).unwrap();
    let a = registry.connect("memory", context("lan", "a"), json!({})).await.unwrap();
    let b = registry.connect("memory", context("lan", "b"), json!({})).await.unwrap();
    let c = registry.connect("memory", context("lan", "c"), json!({})).await.unwrap();
    let isolated = registry.connect("memory", context("other", "b"), json!({})).await.unwrap();
    let frame = Frame::new(Bytes::from(vec![1; 1514])).unwrap();
    a.publish(std::slice::from_ref(&frame)).await.unwrap();
    a.publish(std::slice::from_ref(&frame)).await.unwrap();
    assert!(a.receive(10).await.unwrap().is_empty());
    assert!(isolated.receive(10).await.unwrap().is_empty());
    let deliveries = b.receive(1).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].frame, frame);
    assert_eq!(b.receive(1).await.unwrap()[0].frame, frame);
    b.acknowledge(&[deliveries[0].receipt.clone()]).await.unwrap();
    b.acknowledge(&[deliveries[0].receipt.clone()]).await.unwrap();
    assert!(b.receive(1).await.unwrap().is_empty());
    assert_eq!(c.receive(1).await.unwrap()[0].frame, frame);
    drop(b);
    let b = registry.connect("memory", context("lan", "b"), json!({})).await.unwrap();
    assert!(b.receive(1).await.unwrap().is_empty());
}

#[tokio::test]
async fn full_capacity_rejects_without_deleting_previously_accepted_frames() {
    let mut registry = PluginRegistry::default();
    registry.register(MemoryPlugin::default()).unwrap();
    let a = registry.connect("memory", context("lan", "a"), json!({"capacity":1})).await.unwrap();
    let b = registry.connect("memory", context("lan", "b"), json!({"capacity":1})).await.unwrap();
    let first = Frame::new(Bytes::from(vec![1; 14])).unwrap();
    a.publish(std::slice::from_ref(&first)).await.unwrap();
    let second = Frame::new(Bytes::from(vec![2; 14])).unwrap();
    assert!(a.publish(&[second]).await.unwrap_err().is_retryable());
    assert_eq!(b.receive(2).await.unwrap()[0].frame, first);
}

#[tokio::test]
async fn unknown_plugins_duplicate_registration_and_duplicate_nodes_are_rejected() {
    let mut registry = PluginRegistry::default();
    registry.register(MemoryPlugin::default()).unwrap();
    assert!(registry.register(MemoryPlugin::default()).is_err());
    assert!(registry.connect("unknown", context("lan", "a"), json!({})).await.is_err());
    let _a = registry.connect("memory", context("lan", "a"), json!({})).await.unwrap();
    assert!(registry.connect("memory", context("lan", "a"), json!({})).await.is_err());
}
