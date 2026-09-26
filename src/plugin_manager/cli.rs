use super::{configure::configure, download::download, ManagerResult, Package, PluginStore};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "amitoki plugin", about = "中継プラグインを再ビルドせずに管理する")]
struct Arguments {
    #[arg(long, global = true)]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// GitHub Releaseまたはローカル配布物から追加
    Add {
        source: Option<String>,
        #[arg(long, conflicts_with = "source")]
        path: Option<PathBuf>,
    },
    /// 停止中のプラグインを更新
    Update {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
    },
    /// インストール済みプラグインとバージョンを表示
    List,
    /// 設定項目の定義を表示
    Describe { name: String },
    /// 対話形式または--setで設定
    Configure {
        name: String,
        #[arg(long = "set")]
        assignments: Vec<String>,
    },
    /// 設定を検証（接続しない）
    Validate { name: String },
    /// 停止中のプラグインを削除（設定は保持）
    Remove { name: String },
}

pub async fn run_cli(arguments: &[String]) -> ManagerResult<()> {
    let arguments = Arguments::parse_from(std::iter::once("amitoki plugin".to_owned()).chain(arguments.iter().cloned()));
    let store = match arguments.directory {
        Some(directory) => PluginStore { directory },
        None => PluginStore::from_environment()?,
    };
    match arguments.command {
        Operation::Add { source, path } => {
            let package = if let Some(path) = path {
                store.install(&path, false)?
            } else {
                let source = source.ok_or("プラグイン名、owner/repo、または--pathを指定してください")?;
                let directory = download(&source).await?;
                store.install(directory.path(), false)?
            };
            println!("{} {}を追加しました", package.manifest.name, package.manifest.version);
        },
        Operation::Update { name, path, version } => {
            let installed = Package::load(&store.plugin_path(&name)?)?;
            let downloaded;
            let source = if let Some(path) = path {
                path
            } else {
                let repository = installed.source.ok_or("ローカル配布物は--pathで更新してください")?;
                let source = version.map_or(repository.clone(), |version| format!("{repository}@{version}"));
                downloaded = download(&source).await?;
                downloaded.path().to_owned()
            };
            if Package::load(&source)?.manifest.name != name {
                return Err("更新対象のプラグイン名が一致しません".into());
            }
            let package = store.install(&source, true)?;
            println!("{} {}へ更新しました", name, package.manifest.version);
        },
        Operation::List => {
            println!("memory\t組み込み・テスト用");
            for package in store.list()? {
                println!("{}\t{}", package.manifest.name, package.manifest.version);
            }
        },
        Operation::Describe { name } => println!("{}", serde_json::to_string_pretty(&Package::load(&store.plugin_path(&name)?)?.manifest)?),
        Operation::Configure { name, assignments } => configure(&store, &name, &assignments)?,
        Operation::Validate { name } => {
            Package::load(&store.plugin_path(&name)?)?.manifest.validate_options(&store.options(&name)?)?;
            println!("{name}の設定は正常です（接続は未検証）");
        },
        Operation::Remove { name } => {
            store.remove(&name)?;
            println!("{name}を削除しました（設定は保持）");
        },
    }
    Ok(())
}
