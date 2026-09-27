use serde::Deserialize;
use std::time::Duration;

// Ethernetの最大長でも約64MiB以内に収まる送信待ち件数。
const DEFAULT_QUEUE_CAPACITY: usize = 1024;
// 小パケットをまとめ、SQL往復回数と待ち時間の両方を抑える。
const DEFAULT_BATCH_SIZE: usize = 128;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 10;
const DEFAULT_POLL_INTERVAL_MS: u64 = 10;
// 障害時に中継先へ接続要求を送り続けないための待機時間。
const DEFAULT_RETRY_INTERVAL_MS: u64 = 100;
const DEFAULT_OPERATION_TIMEOUT_MS: u64 = 10_000;
// 通常の操作タイムアウトより長く取り、停止時にバッファを送れるようにする。
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 30_000;
// 新世代の全Stageのコピー・検証・初期化に使う上限。
const DEFAULT_RELOAD_TIMEOUT_MS: u64 = 30_000;
// 誤設定による極端なメモリ確保や停止待ちを拒否する。
const MAX_QUEUE_CAPACITY: usize = 65_536;
const MAX_BATCH_SIZE: usize = 4096;
const MAX_INTERVAL_MS: u64 = 300_000;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub queue_capacity: usize,
    pub relay_queue_capacity: usize,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub poll_interval_ms: u64,
    pub retry_interval_ms: u64,
    pub operation_timeout_ms: u64,
    pub shutdown_timeout_ms: u64,
    pub reload_timeout_ms: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            relay_queue_capacity: DEFAULT_QUEUE_CAPACITY,
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval_ms: DEFAULT_FLUSH_INTERVAL_MS,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            retry_interval_ms: DEFAULT_RETRY_INTERVAL_MS,
            operation_timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
            shutdown_timeout_ms: DEFAULT_SHUTDOWN_TIMEOUT_MS,
            reload_timeout_ms: DEFAULT_RELOAD_TIMEOUT_MS,
        }
    }
}

impl EngineConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=MAX_QUEUE_CAPACITY).contains(&self.queue_capacity)
            || !(1..=MAX_QUEUE_CAPACITY).contains(&self.relay_queue_capacity)
            || !(1..=MAX_BATCH_SIZE).contains(&self.batch_size)
        {
            return Err("queue_capacityとrelay_queue_capacityは1〜65536、batch_sizeは1〜4096で指定してください");
        }
        let intervals = [
            self.flush_interval_ms,
            self.poll_interval_ms,
            self.retry_interval_ms,
            self.operation_timeout_ms,
            self.shutdown_timeout_ms,
            self.reload_timeout_ms,
        ];
        if intervals.iter().any(|value| !(1..=MAX_INTERVAL_MS).contains(value)) {
            return Err("待機時間は1〜300000ミリ秒で指定してください");
        }
        Ok(())
    }

    pub(crate) fn reload_timeout(&self) -> Duration {
        Duration::from_millis(self.reload_timeout_ms)
    }
    pub(crate) fn operation_timeout(&self) -> Duration {
        Duration::from_millis(self.operation_timeout_ms)
    }
    pub(crate) fn retry_interval(&self) -> Duration {
        Duration::from_millis(self.retry_interval_ms)
    }
}
