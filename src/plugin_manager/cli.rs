use super::{
    configure::configure,
    operations::{self, InstallOptions},
    source::{expand_path, PluginTarget},
    ManagerResult, PluginKind, PluginStore,
};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "amitoki plugin", about = "プラグインを再ビルドせずに管理・検証する")]
struct Arguments {
    #[arg(long, global = true)]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: RootOperation,
}

#[derive(Subcommand)]
enum RootOperation {
    /// 通信を中継するプラグイン
    Relay(Operations),
    /// 解析・フィルタ・分岐を行うプラグイン
    #[command(name = "stage", alias = "block")]
    Block(Operations),
    #[command(flatten)]
    Legacy(Operation),
}

#[derive(Args)]
struct Operations {
    #[command(subcommand)]
    command: Operation,
}

#[derive(Subcommand)]
enum Operation {
    /// GitHub Releaseまたはローカル配布物から追加
    Add {
        target: Option<String>,
        #[arg(long, conflicts_with = "target")]
        path: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
    },
    /// Stageは稼働中も更新できる。反映にはreloadが必要
    Update {
        target: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
    },
    /// インストール済みプラグインとバージョンを表示
    List,
    /// 設定項目の定義を表示
    Describe { target: String },
    /// 対話形式または--setで設定
    #[command(alias = "config")]
    Configure {
        target: String,
        #[arg(long = "set")]
        assignments: Vec<String>,
    },
    /// 設定を検証（接続しない）
    Validate { target: String },
    /// 登録を削除（設定は保持、稼働中のStageは継続）
    #[command(name = "del", alias = "remove")]
    Remove { target: String },
    /// PCAPまたはRust定義の生成パケットでStageを検証
    Test(crate::debug::BlockTestArguments),
    /// 生成パケットでStageとIPCの処理速度・バッチ遅延を測定
    Bench(crate::debug::bench::BenchArguments),
    /// ソース変更後にビルドし、PCAP単体テストを繰り返す
    Watch(crate::debug::watch::WatchArguments),
}

pub async fn run_cli(arguments: &[String]) -> ManagerResult<()> {
    let arguments = Arguments::parse_from(std::iter::once("amitoki plugin".to_owned()).chain(arguments.iter().cloned()));
    let store = match arguments.directory {
        Some(directory) => PluginStore {
            directory: expand_path(&directory)?,
        },
        None => PluginStore::from_environment()?,
    };
    let (kind, operation) = match arguments.command {
        RootOperation::Relay(operations) => (Some(PluginKind::Relay), operations.command),
        RootOperation::Block(operations) => (Some(PluginKind::Block), operations.command),
        RootOperation::Legacy(operation) => (None, operation),
    };
    match operation {
        Operation::Add { target, path, version } => operations::add(&store, InstallOptions { kind, target, path, version }).await?,
        Operation::Update { target, path, version } => {
            operations::update(
                &store,
                InstallOptions {
                    kind,
                    target: Some(target),
                    path,
                    version,
                },
            )
            .await?
        },
        Operation::List => {
            if kind != Some(PluginKind::Block) {
                println!("memory\trelay\t組み込み・テスト用");
            }
            for package in store.list()? {
                let actual = PluginKind::of(&package);
                if kind.is_none_or(|kind| kind == actual) {
                    println!("{}\t{}\t{}", package.manifest.name, actual.name(), package.manifest.version);
                }
            }
        },
        Operation::Describe { target } => {
            let package = PluginTarget::parse(&target)?.installed(&store, kind)?;
            println!("{}", serde_json::to_string_pretty(&package.manifest)?);
        },
        Operation::Configure { target, assignments } => {
            let package = PluginTarget::parse(&target)?.installed(&store, kind)?;
            configure(&store, &package.manifest.name, &assignments)?;
        },
        Operation::Validate { target } => {
            let package = PluginTarget::parse(&target)?.installed(&store, kind)?;
            package.manifest.validate_options(&store.options(&package.manifest.name)?)?;
            println!("{}の設定は正常です（接続は未検証）", package.manifest.name);
        },
        Operation::Remove { target } => {
            let package = PluginTarget::parse(&target)?.installed(&store, kind)?;
            store.remove(&package.manifest.name)?;
            println!("{}を削除しました（設定は保持）", package.manifest.name);
        },
        Operation::Watch(arguments) => {
            if kind != Some(PluginKind::Block) {
                return Err("watchはplugin stage watchで実行してください".into());
            }
            crate::debug::watch::watch(&store, arguments).await?;
        },
        Operation::Bench(arguments) => {
            if kind == Some(PluginKind::Relay) {
                return Err("benchはStage用です".into());
            }
            crate::debug::bench::run(&store, arguments).await?;
        },
        Operation::Test(arguments) => {
            if kind != Some(PluginKind::Block) {
                return Err("単体テストはplugin stage testで実行してください".into());
            }
            crate::debug::test_block(&store, arguments).await?;
        },
    }
    Ok(())
}
