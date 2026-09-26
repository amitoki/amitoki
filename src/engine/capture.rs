use super::{Engine, EngineError};
use amitoki_relay::{Frame, MAX_FRAME_SIZE};
use bytes::Bytes;
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(super) async fn capture(engine: Arc<Engine>, sender: mpsc::Sender<Frame>, shutdown: CancellationToken) -> Result<(), EngineError> {
    // 受信バッファは使い回し、キューには実際のフレーム長だけコピーする。
    let mut buffer = vec![0; MAX_FRAME_SIZE];
    loop {
        let permit = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            permit = sender.reserve() => permit.map_err(|_| EngineError::Worker("送信キューが閉じました".into()))?,
        };
        let length = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            received = engine.network.receive(&mut buffer) => received?,
        };
        let bytes = buffer.get(..length).ok_or_else(|| EngineError::Worker("受信サイズがバッファを超えています".into()))?;
        let allowed = engine.firewall.check_frame(bytes).is_ok();
        if !allowed {
            engine.metrics.filtered.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let frame = Frame::new(Bytes::copy_from_slice(bytes))?;
        engine.metrics.captured.fetch_add(1, Ordering::Relaxed);
        permit.send(frame);
    }
}
