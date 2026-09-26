//! 同一プロセス内で使う有限長の参照実装。外部サービスやグローバル状態は使わない。
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError, RelayPlugin};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};
use uuid::Uuid;

// テストで保持可能な上限。満杯時は消さずにバックプレッシャを返す。
const DEFAULT_CAPACITY: usize = 4096;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    capacity: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { capacity: DEFAULT_CAPACITY }
    }
}

struct StoredFrame {
    source: String,
    frame: Frame,
}

struct Channel {
    capacity: usize,
    frames: Vec<StoredFrame>,
    frame_ids: HashSet<Uuid>,
    acknowledgements: HashMap<String, HashSet<Uuid>>,
    active_nodes: HashSet<String>,
}

#[derive(Default)]
pub struct MemoryPlugin {
    channels: Arc<Mutex<HashMap<String, Channel>>>,
}

struct MemoryRelay {
    context: RelayContext,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
}

fn lock_channels(channels: &Mutex<HashMap<String, Channel>>) -> Result<MutexGuard<'_, HashMap<String, Channel>>, RelayError> {
    channels.lock().map_err(|_| RelayError::permanent("メモリ中継のロックが破損しています"))
}

#[async_trait]
impl RelayPlugin for MemoryPlugin {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn connect(&self, context: RelayContext, options: Value) -> Result<Arc<dyn Relay>, RelayError> {
        context.validate()?;
        let options: Options = serde_json::from_value(options).map_err(|_| RelayError::permanent("memoryの設定が不正です"))?;
        if options.capacity == 0 {
            return Err(RelayError::permanent("memory.capacityには1以上を指定してください"));
        }
        let mut channels = lock_channels(&self.channels)?;
        let channel = channels.entry(context.channel.clone()).or_insert_with(|| Channel {
            capacity: options.capacity,
            frames: Vec::new(),
            frame_ids: HashSet::new(),
            acknowledgements: HashMap::new(),
            active_nodes: HashSet::new(),
        });
        if channel.capacity != options.capacity {
            return Err(RelayError::permanent("同じchannelには同じcapacityを指定してください"));
        }
        if !channel.active_nodes.insert(context.node_id.clone()) {
            return Err(RelayError::permanent("同じノードが既に接続しています"));
        }
        channel.acknowledgements.entry(context.node_id.clone()).or_default();
        Ok(Arc::new(MemoryRelay {
            context,
            channels: self.channels.clone(),
        }))
    }
}

impl Drop for MemoryRelay {
    fn drop(&mut self) {
        if let Ok(mut channels) = self.channels.lock() {
            if let Some(channel) = channels.get_mut(&self.context.channel) {
                channel.active_nodes.remove(&self.context.node_id);
            }
        }
    }
}

#[async_trait]
impl Relay for MemoryRelay {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        for frame in frames {
            frame.validate()?;
        }
        let mut channels = lock_channels(&self.channels)?;
        let channel = channels.get_mut(&self.context.channel).expect("接続中のchannel");
        let new_ids: HashSet<_> = frames.iter().map(|frame| frame.id).filter(|id| !channel.frame_ids.contains(id)).collect();
        if channel.frames.len() + new_ids.len() > channel.capacity {
            return Err(RelayError::retryable("メモリ中継が容量上限に達しました"));
        }
        for frame in frames {
            if channel.frame_ids.insert(frame.id) {
                channel.frames.push(StoredFrame {
                    source: self.context.node_id.clone(),
                    frame: frame.clone(),
                });
            }
        }
        Ok(())
    }

    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        let channels = lock_channels(&self.channels)?;
        let channel = &channels[&self.context.channel];
        let acknowledged = &channel.acknowledgements[&self.context.node_id];
        Ok(channel
            .frames
            .iter()
            .filter(|stored| stored.source != self.context.node_id && !acknowledged.contains(&stored.frame.id))
            .take(limit)
            .map(|stored| Delivery {
                frame: stored.frame.clone(),
                receipt: Receipt(stored.frame.id.to_string()),
            })
            .collect())
    }

    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        let ids: Result<Vec<_>, _> = receipts.iter().map(|receipt| Uuid::parse_str(&receipt.0)).collect();
        let ids = ids.map_err(|_| RelayError::permanent("memoryの受領情報が不正です"))?;
        let mut channels = lock_channels(&self.channels)?;
        let channel = channels.get_mut(&self.context.channel).expect("接続中のchannel");
        let known_ids: Vec<_> = ids.into_iter().filter(|id| channel.frame_ids.contains(id)).collect();
        channel.acknowledgements.get_mut(&self.context.node_id).expect("接続中のノード").extend(known_ids);
        Ok(())
    }
}
