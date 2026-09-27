use crate::debug::BlockTestArguments;
use clap::Args;
use std::time::Duration;

// 起動直後の初期化を測定区間から外す。利用者は0にも設定できる。
const DEFAULT_WARMUP: u64 = 128;

#[derive(Args)]
pub struct BenchArguments {
    #[command(flatten)]
    pub(super) test: BlockTestArguments,
    /// 件数の代わりに実行時間を指定（例: 30s）。終了時に処理中のバッチは完了させる
    #[arg(long, conflicts_with = "count", value_parser = parse_duration)]
    pub(super) duration: Option<Duration>,
    /// 毎秒の投入件数。省略時は待機しない
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=1_000_000_000))]
    pub(super) rate: Option<u64>,
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u16).range(1..=128))]
    pub(super) batch_size: u16,
    #[arg(long, default_value_t = DEFAULT_WARMUP, value_parser = clap::value_parser!(u64).range(0..=1_000_000))]
    pub(super) warmup: u64,
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let seconds: u64 = value.strip_suffix('s').unwrap_or(value).parse().map_err(|_| "秒数で指定してください（例: 30s）")?;
    if !(1..=3600).contains(&seconds) {
        return Err("実行時間は1〜3600秒で指定してください".into());
    }
    Ok(Duration::from_secs(seconds))
}
