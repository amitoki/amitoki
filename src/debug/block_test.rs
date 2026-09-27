use super::{
    generated::GeneratedInput,
    pcap::CapturePacket,
    report::PacketReport,
    runner::{ReplayInput, ReplayReader},
    stage_session::StageSession,
    BlockTestArguments,
};
use crate::plugin_manager::{source::expand_path, ManagerResult, PluginStore};
use amitoki_plugin_sdk::wire::MAX_BATCH;

pub async fn test_block(store: &PluginStore, arguments: BlockTestArguments) -> ManagerResult<()> {
    let session = StageSession::open(store, &arguments, &arguments.target).await?;
    let plan = session.plan();
    if let Some(pcap) = &arguments.pcap {
        let reader = ReplayReader::open(ReplayInput {
            pcap: expand_path(pcap)?,
            source: arguments.instance.clone(),
            json: arguments.json,
        })?;
        return plan.run(reader).await;
    }
    let generator = GeneratedInput::open(store, &arguments, &session).await?;
    let mut index = 0;
    let mut rejected = 0;
    while index < arguments.count {
        let count = (arguments.count - index).min(MAX_BATCH as u64) as usize;
        for bytes in generator.generate(index, count).await? {
            index += 1;
            let capture = CapturePacket {
                original_length: bytes.len() as u32,
                bytes,
                timestamp_ns: 0,
            };
            let mut report = PacketReport::new(index, &arguments.instance, &capture);
            let outcome = plan.process_capture(capture, &mut report).await;
            rejected += u64::from(report.rejection.is_some());
            report.write(&mut std::io::stdout().lock(), arguments.json)?;
            outcome?;
        }
    }
    if rejected != 0 {
        return Err(format!("生成パケット{rejected}件が本体検査で拒否されました").into());
    }
    Ok(())
}
