use crate::{
    wire::{read_message, write_message, Request, Response, MAX_BATCH},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::{Relay, RelayError, RelayPlugin};
use std::{io, sync::Arc};

pub async fn serve(plugin: impl RelayPlugin, manifest: PluginManifest) -> Result<(), Box<dyn std::error::Error>> {
    manifest.validate()?;
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let mut relay = None;
    loop {
        let request = match read_message(&mut input).await {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let response = dispatch((&plugin, &manifest), &mut relay, request).await.unwrap_or_else(Response::from);
        write_message(&mut output, &response).await?;
    }
}

async fn dispatch(definition: (&impl RelayPlugin, &PluginManifest), relay: &mut Option<Arc<dyn Relay>>, request: Request) -> Result<Response, RelayError> {
    let (plugin, manifest) = definition;
    match request {
        Request::Describe => Ok(Response::Manifest(manifest.clone())),
        Request::Connect {
            protocol_version,
            context,
            options,
        } => {
            if protocol_version != PROTOCOL_VERSION || relay.is_some() {
                return Err(RelayError::permanent("通信仕様が不一致、または接続済みです"));
            }
            manifest.validate_options(&options)?;
            *relay = Some(plugin.connect(context, options).await?);
            Ok(Response::Success)
        },
        request => {
            let relay = relay.as_ref().ok_or_else(|| RelayError::permanent("プラグインが未接続です"))?;
            dispatch_connected(relay.as_ref(), request).await
        },
    }
}

async fn dispatch_connected(relay: &dyn Relay, request: Request) -> Result<Response, RelayError> {
    match request {
        Request::Publish { frames } if frames.len() <= MAX_BATCH => {
            for frame in &frames {
                frame.validate()?;
            }
            relay.publish(&frames).await?;
            Ok(Response::Success)
        },
        Request::Receive { limit } if limit <= MAX_BATCH => {
            let deliveries = relay.receive(limit).await?;
            if deliveries.len() > limit {
                return Err(RelayError::permanent("プラグインが受信上限を超えました"));
            }
            for delivery in &deliveries {
                delivery.frame.validate()?;
            }
            Ok(Response::Deliveries(deliveries))
        },
        Request::Acknowledge { receipts } if receipts.len() <= MAX_BATCH => {
            relay.acknowledge(&receipts).await?;
            Ok(Response::Success)
        },
        _ => Err(RelayError::permanent("操作またはバッチ件数が不正です")),
    }
}
