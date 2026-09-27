use amitoki::{config::AppConfig, pipeline::graph::Graph};
use amitoki_plugin_sdk::{
    pipeline::{Pipeline, RelayInstance, Stage},
    stage::StageDefinition,
};

#[test]
fn rust_connections_determine_stage_order_independently_of_registration_order() {
    let pipeline = Pipeline::new()
        .add_stage(Stage::plugin("second", "packet-rules"))
        .add_stage(Stage::plugin("first", "packet-rules"))
        .add_relay(RelayInstance::plugin("database", "memory"))
        .connect("capture", ["first"])
        .connect("first.pass", ["second"])
        .connect("second.pass", ["database"])
        .discard("first.drop")
        .discard("second.drop")
        .connect("database.received", ["inject"])
        .build()
        .unwrap();
    let definition = StageDefinition {
        rewrite: false,
        outputs: vec!["pass".into(), "drop".into()],
    };
    let graph = Graph::compile(&pipeline, &[definition.clone(), definition]).unwrap();
    assert_eq!(graph.order, vec![1, 0]);
}

#[test]
fn a_generated_pipeline_is_loaded_relative_to_the_operational_configuration() {
    let directory = tempfile::tempdir().unwrap();
    Pipeline::new().discard("capture").write(directory.path().join("pipeline.json")).unwrap();
    let config = directory.path().join("amitoki.toml");
    std::fs::write(&config, "node_id='a'\nchannel='lab'\ninterface='fake0'\npipeline_file='pipeline.json'\n").unwrap();
    let loaded = AppConfig::load(&config).unwrap();
    assert_eq!(loaded.pipeline.unwrap().routes[0].from, "capture");
    std::fs::write(
        &config,
        "node_id='a'\nchannel='lab'\ninterface='fake0'\npipeline_file='pipeline.json'\n[relay]\nplugin='memory'\n",
    )
    .unwrap();
    assert!(AppConfig::load(&config).is_err());
}
