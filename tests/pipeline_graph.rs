use amitoki::pipeline::{graph::Graph, PipelineConfig};
use amitoki_plugin_sdk::block::BlockDefinition;
use serde_json::{json, Value};

fn configuration(routes: Value) -> PipelineConfig {
    serde_json::from_value(json!({"relays":[{"id":"db","plugin":"memory"},{"id":"peer","plugin":"memory","channel":"other"}],"blocks":[{"id":"classify","plugin":"packet-rules"},{"id":"observe","plugin":"packet-rules"}],"routes":routes})).unwrap()
}
fn definitions() -> Vec<BlockDefinition> {
    vec![
        BlockDefinition {
            rewrite: false,
            outputs: vec!["pass".into(), "drop".into()]
        };
        2
    ]
}
fn routes() -> Value {
    json!([
        {"from":"capture","to":["classify"]},
        {"from":"classify.pass","to":["db","peer","observe"]},
        {"from":"classify.drop","to":[]},
        {"from":"observe.pass","to":[]},
        {"from":"observe.drop","to":[]},
        {"from":"db.received","to":["inject"]},
        {"from":"peer.received","to":["inject"]}
    ])
}
#[test]
fn multiple_relays_and_an_optional_observation_branch_can_be_connected() {
    assert!(Graph::compile(&configuration(routes()), &definitions()).is_ok());
}
#[test]
fn cycles_are_rejected_before_plugins_start() {
    let mut routes = routes();
    routes[3]["to"] = json!(["classify"]);
    assert!(Graph::compile(&configuration(routes), &definitions()).unwrap_err().contains("循環"));
}
#[test]
fn received_frames_cannot_reach_any_relay_even_through_custom_blocks() {
    let mut routes = routes();
    routes[5]["to"] = json!(["classify"]);
    assert!(Graph::compile(&configuration(routes), &definitions()).unwrap_err().contains("再転送"));
}
#[test]
fn capture_cannot_be_reinjected_even_through_an_analysis_branch() {
    let mut routes = routes();
    routes[3]["to"] = json!(["inject"]);
    assert!(Graph::compile(&configuration(routes), &definitions()).unwrap_err().contains("inject"));
}
#[test]
fn unknown_ports_missing_routes_and_duplicate_ids_are_rejected() {
    let mut missing = routes();
    missing.as_array_mut().unwrap().remove(2);
    assert!(Graph::compile(&configuration(missing), &definitions()).is_err());
    let mut unknown = routes();
    unknown[2]["from"] = json!("classify.typo");
    assert!(Graph::compile(&configuration(unknown), &definitions()).is_err());
    let mut duplicate = configuration(routes());
    duplicate.blocks[1].id = "db".into();
    assert!(Graph::compile(&duplicate, &definitions()).is_err());
}
#[test]
fn exponential_fan_out_is_rejected_even_when_the_graph_has_no_cycle() {
    let blocks: Vec<_> = (0..9).map(|index| json!({"id":format!("block{index}"),"plugin":"test"})).collect();
    let mut routes = vec![json!({"from":"capture","to":["block0"]})];
    for index in 0..9 {
        for port in ["pass", "drop"] {
            routes.push(json!({"from":format!("block{index}.{port}"),"to":if index == 8 { vec![] } else { vec![format!("block{}", index+1)] }}));
        }
    }
    let config: PipelineConfig = serde_json::from_value(json!({"blocks":blocks,"routes":routes})).unwrap();
    let definitions = vec![definitions()[0].clone(); 9];
    assert!(Graph::compile(&config, &definitions).unwrap_err().contains("128"));
}
