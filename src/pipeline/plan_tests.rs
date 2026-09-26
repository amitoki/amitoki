use super::*;
use crate::pipeline::PipelineConfig;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;

struct Annotate;
struct Select;
#[async_trait]
impl Block for Annotate {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        Ok(packets
            .iter()
            .map(|packet| BlockOutput {
                ports: vec!["out".into()],
                annotations: json!({"allowed":packet.frame.bytes[0]==1,"id":"cannot-change-the-frame-id"}),
            })
            .collect())
    }
}
#[async_trait]
impl Block for Select {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        Ok(packets
            .iter()
            .map(|packet| BlockOutput {
                ports: if packet.annotations["allowed"] == true { vec!["out".into()] } else { vec![] },
                annotations: packet.annotations.clone(),
            })
            .collect())
    }
}
fn config(merge: bool) -> PipelineConfig {
    serde_json::from_value(
        json!({"relays":[{"id":"db","plugin":"memory"}],"blocks":[{"id":"analyse","plugin":"custom"},{"id":"filter","plugin":"custom"}],"routes":[
            {"from":"capture","to":["analyse"]},{"from":"analyse.out","to":if merge {vec!["filter","db"]} else {vec!["filter"]}},
            {"from":"filter.out","to":["db"]},{"from":"db.received","to":[]}
        ]}),
    )
    .unwrap()
}
fn blocks() -> Vec<RunningBlock> {
    [Arc::new(Annotate) as Arc<dyn Block>, Arc::new(Select) as Arc<dyn Block>]
        .into_iter()
        .map(|block| RunningBlock {
            block,
            definition: BlockDefinition { outputs: vec!["out".into()] },
            on_error: ErrorPolicy::Stop,
        })
        .collect()
}
#[tokio::test]
async fn analysis_metadata_can_drive_a_later_filter_without_changing_frame_identity_or_bytes() {
    let blocks = blocks();
    let graph = Graph::compile(&config(false), &blocks.iter().map(|block| block.definition.clone()).collect::<Vec<_>>()).unwrap();
    let metrics = PipelineMetrics::default();
    let planner = Planner {
        graph: &graph,
        blocks: &blocks,
        metrics: &metrics,
        operation_timeout: Duration::from_secs(1),
    };
    let frames: Vec<_> = (1..=2).map(|value| Frame::new(Bytes::from(vec![value; 14])).unwrap()).collect();
    let plan = planner.prepare(&frames, &graph.capture).await.unwrap();
    assert_eq!(plan.relays[0], vec![frames[0].clone()]);
    assert_eq!(metrics.processed.load(Ordering::Relaxed), 4);
}
#[tokio::test]
async fn reconverging_branches_publish_the_same_frame_once_to_each_terminal() {
    let blocks = blocks();
    let graph = Graph::compile(&config(true), &blocks.iter().map(|block| block.definition.clone()).collect::<Vec<_>>()).unwrap();
    let metrics = PipelineMetrics::default();
    let planner = Planner {
        graph: &graph,
        blocks: &blocks,
        metrics: &metrics,
        operation_timeout: Duration::from_secs(1),
    };
    let frame = Frame::new(Bytes::from(vec![1; 14])).unwrap();
    let plan = planner.prepare(std::slice::from_ref(&frame), &graph.capture).await.unwrap();
    assert_eq!(plan.relays[0], vec![frame]);
}
