use crate::{
    process::ProcessClient,
    wire::{Request, Response, MAX_BATCH},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct ProcessRelay {
    process: ProcessClient,
}
impl ProcessRelay {
    pub async fn connect(executable: &Path, manifest: &PluginManifest, configuration: (RelayContext, Value)) -> Result<Self, RelayError> {
        let (context, options) = configuration;
        manifest.validate_options(&options)?;
        if manifest.block.is_some() {
            return Err(RelayError::permanent("解析ブロックは中継として使えません"));
        }
        let process = ProcessClient::start(executable, manifest).await?;
        process
            .success(Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                context,
                options,
            })
            .await?;
        Ok(Self { process })
    }
}

#[async_trait]
impl Relay for ProcessRelay {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        for frame in frames {
            frame.validate()?;
        }
        for batch in frames.chunks(MAX_BATCH) {
            self.process.success(Request::Publish { frames: batch.to_vec() }).await?;
        }
        Ok(())
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        let limit = limit.min(MAX_BATCH);
        match self.process.call(Request::Receive { limit }).await? {
            Response::Deliveries(deliveries) if deliveries.len() <= limit => {
                for delivery in &deliveries {
                    delivery.frame.validate()?;
                }
                Ok(deliveries)
            },
            _ => Err(RelayError::permanent("プラグインの受信応答が不正です")),
        }
    }
    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        for batch in receipts.chunks(MAX_BATCH) {
            self.process.success(Request::Acknowledge { receipts: batch.to_vec() }).await?;
        }
        Ok(())
    }
}
