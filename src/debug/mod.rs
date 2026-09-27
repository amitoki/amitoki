//! NICと中継を起動せず、保存パケットで実際のブロック処理を検証する。
pub mod bench;
mod block_test;
mod compare;
mod generated;
mod pcap;
mod replay;
mod report;
mod runner;
mod stage_session;
pub mod watch;

use crate::plugin_manager::{source::expand_path, ManagerResult, PluginStore};
pub use block_test::test_block;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct BlockTestArguments {
    /// 配布ディレクトリ、追加済みの名前またはGitHub URL
    pub target: String,
    #[arg(long, conflicts_with = "packet", required_unless_present = "packet")]
    pub pcap: Option<PathBuf>,
    /// Rustで定義したパケット生成器の名前
    #[arg(long, conflicts_with = "pcap", required_unless_present = "pcap")]
    pub packet: Option<String>,
    /// 生成器を提供する別のStageプラグイン（省略時はテスト対象）
    #[arg(long, requires = "packet")]
    pub generator: Option<String>,
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(1..=1_000_000_000))]
    pub count: u64,
    #[arg(long, default_value_t = 42)]
    pub seed: u64,
    /// パケットの生成条件
    #[arg(long = "packet-set", requires = "packet")]
    pub packet_assignments: Vec<String>,
    /// 保存済みの設定を変更せず、このテストだけ上書き
    #[arg(long = "set")]
    pub assignments: Vec<String>,
    #[arg(long, default_value = "debug")]
    pub node_id: String,
    #[arg(long, default_value = "debug")]
    pub channel: String,
    #[arg(long, default_value = "test")]
    pub instance: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser)]
#[command(name = "amitoki debug", about = "保存パケットでブロックを検証し、経路・解析結果を比較する")]
struct Arguments {
    #[arg(long, global = true)]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// 本番と同じ経路判定を行い、送信予定だけを表示（NIC・中継は起動しない）
    Replay {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        pcap: PathBuf,
        #[arg(long, default_value = "capture")]
        source: String,
        #[arg(long)]
        json: bool,
    },
    /// --jsonの結果を比較（処理時間は比較しない、差分ありは終了コード1）
    Compare {
        before: PathBuf,
        after: PathBuf,
    },
}

pub async fn run_cli(arguments: &[String]) -> ManagerResult<()> {
    let arguments = Arguments::parse_from(std::iter::once("amitoki debug".to_owned()).chain(arguments.iter().cloned()));
    let store = match arguments.directory {
        Some(directory) => PluginStore {
            directory: expand_path(&directory)?,
        },
        None => PluginStore::from_environment()?,
    };
    match arguments.command {
        Operation::Replay { config, pcap, source, json } => {
            replay::replay(
                &store,
                &expand_path(&config)?,
                runner::ReplayInput {
                    pcap: expand_path(&pcap)?,
                    source,
                    json,
                },
            )
            .await
        },
        Operation::Compare { before, after } => compare::compare(&expand_path(&before)?, &expand_path(&after)?),
    }
}
