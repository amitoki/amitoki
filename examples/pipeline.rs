//! cargo run --example pipeline -- ./pipeline.json
use amitoki_plugin_sdk::pipeline::{Pipeline, RelayInstance, Stage};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).ok_or("出力するPipelineファイルを指定してください")?;
    Pipeline::new()
        .add_stage(Stage::plugin("parse", "telemetry").options(json!({"operation":"parse"})))
        .add_stage(Stage::plugin("filter", "telemetry").options(json!({"operation":"filter"})))
        .add_stage(Stage::plugin("observe", "telemetry").options(json!({"operation":"log"})))
        // ラボ用のmemory。実際の中継では登録済みのpostgres/p2pとその設定を指定する。
        .add_relay(RelayInstance::plugin("sink", "memory"))
        .connect("capture", ["parse"])
        .connect("parse.pass", ["filter"])
        .connect("filter.pass", ["observe"])
        .connect("observe.pass", ["sink"])
        .discard("parse.drop")
        .discard("filter.drop")
        .discard("observe.drop")
        .connect("sink.received", ["inject"])
        .write(std::path::Path::new(&output))
}
