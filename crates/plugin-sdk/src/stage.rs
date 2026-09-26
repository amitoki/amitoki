//! StageはPipeline内の処理単位。既存プラグインのwire形式は維持する。
pub use crate::block::{
    serve_block as serve_stage, Block as Stage, BlockContext as StageContext, BlockDefinition as StageDefinition, BlockOutput as StageOutput, BlockPacket as StagePacket,
    BlockPlugin as StagePlugin, ProcessBlock as ProcessStage,
};
