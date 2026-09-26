use super::report::PacketReport;
use crate::plugin_manager::ManagerResult;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

// 128訪問×4096バイトの解析結果と経路情報を収め、壊れた行で無制限に確保しない。
const MAX_REPORT_BYTES: u64 = 2 * 1024 * 1024;

pub(super) fn compare(before: &Path, after: &Path) -> ManagerResult<()> {
    let mut before = BufReader::new(File::open(before)?);
    let mut after = BufReader::new(File::open(after)?);
    let mut changed = 0;
    let mut packets = 0;
    loop {
        let previous = read_report(&mut before)?.map(PacketReport::without_timing);
        let current = read_report(&mut after)?.map(PacketReport::without_timing);
        if previous.is_none() && current.is_none() {
            break;
        }
        packets += 1;
        if previous != current {
            changed += 1;
            println!("#{packets}: 差分あり");
            for (label, report) in [("before", previous), ("after", current)] {
                println!("{label}: {}", serde_json::to_string(&report)?);
            }
        }
    }
    println!("{packets}件を比較、{changed}件に差分（処理時間を除く）");
    if changed != 0 {
        return Err("再生結果に差分があります".into());
    }
    Ok(())
}

fn read_report(reader: &mut impl BufRead) -> ManagerResult<Option<PacketReport>> {
    let mut line = String::new();
    if reader.take(MAX_REPORT_BYTES + 1).read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.len() as u64 > MAX_REPORT_BYTES {
        return Err("再生結果の行が大きすぎます".into());
    }
    Ok(Some(serde_json::from_str(&line)?))
}
