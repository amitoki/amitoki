use super::{valid_identifier, Block, BlockPlugin};
use crate::{
    wire::{read_message, write_message, Request, Response, MAX_BATCH},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::RelayError;
use std::{io, sync::Arc};

pub async fn serve_block(plugin: impl BlockPlugin, manifest: PluginManifest) -> Result<(), Box<dyn std::error::Error>> {
    manifest.validate()?;
    if manifest.block.is_none() {
        return Err("ブロックの定義がありません".into());
    }
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let mut block = None;
    loop {
        let request = match read_message(&mut input).await {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let response = dispatch((&plugin, &manifest), &mut block, request).await.unwrap_or_else(Response::from);
        write_message(&mut output, &response).await?;
    }
}

async fn dispatch(definition: (&impl BlockPlugin, &PluginManifest), block: &mut Option<Arc<dyn Block>>, request: Request) -> Result<Response, RelayError> {
    let (plugin, manifest) = definition;
    match request {
        Request::Describe => Ok(Response::Manifest(manifest.clone())),
        Request::ConnectBlock {
            protocol_version,
            context,
            options,
        } => {
            if protocol_version != PROTOCOL_VERSION || block.is_some() || !valid_identifier(&context.instance) {
                return Err(RelayError::permanent("通信仕様・インスタンス名が不正、または接続済みです"));
            }
            context.relay.validate()?;
            manifest.validate_options(&options)?;
            *block = Some(plugin.connect(context, options).await?);
            Ok(Response::Success)
        },
        Request::Process { packets } if packets.len() <= MAX_BATCH => {
            for packet in &packets {
                packet.validate()?;
            }
            let block = block.as_ref().ok_or_else(|| RelayError::permanent("ブロックが未接続です"))?;
            let outputs = block.process(&packets).await?;
            if outputs.len() != packets.len() {
                return Err(RelayError::permanent("解析結果の件数が一致しません"));
            }
            for output in &outputs {
                output.validate(manifest.block.as_ref().expect("validated block"))?;
            }
            Ok(Response::Processed(outputs))
        },
        _ => Err(RelayError::permanent("ブロックの操作またはバッチ件数が不正です")),
    }
}
