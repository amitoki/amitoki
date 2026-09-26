mod changes;
mod process;

use super::BlockTestArguments;
use crate::plugin_manager::{
    source::{absolute_directory, expand_path, PluginTarget},
    ManagerResult,
};
use changes::Changes;
use clap::Args;
use process::ProcessCommand;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

// エディタの複数回書き込みをまとめる。実行中の変更は次の実行まで保持する。
const DEFAULT_DEBOUNCE_MS: u64 = 200;
// ビルドが停止しても永続的にwatchを占有させない。CLIで最大1時間まで指定可能。
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
const MAX_TIMEOUT_SECONDS: u64 = 3600;
const MAX_DEBOUNCE_MS: u64 = 10_000;

#[derive(Args)]
pub struct WatchArguments {
    #[command(flatten)]
    pub test: BlockTestArguments,
    /// 監視するソースファイル・ディレクトリ（複数指定可）
    #[arg(long = "watch")]
    paths: Vec<PathBuf>,
    /// 実行するビルドプログラムまたはスクリプト（シェル展開しない）
    #[arg(long, requires = "paths")]
    build: Option<PathBuf>,
    /// ビルドプログラムに渡す引数。--build-arg=--releaseのように指定
    #[arg(long = "build-arg", requires = "build")]
    build_arguments: Vec<OsString>,
    /// 変更を無視するパス（複数指定可）
    #[arg(long = "exclude")]
    excluded: Vec<PathBuf>,
    #[arg(long, default_value_t = DEFAULT_DEBOUNCE_MS, value_parser = clap::value_parser!(u64).range(1..=MAX_DEBOUNCE_MS))]
    debounce_ms: u64,
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..=MAX_TIMEOUT_SECONDS))]
    timeout_seconds: u64,
    /// 最初のビルドとテストだけ実行して終了（CI用）
    #[arg(long)]
    once: bool,
}

struct WatchSession {
    test: ProcessCommand,
    build: Option<ProcessCommand>,
    changes: Changes,
    debounce: Duration,
    once: bool,
}

pub async fn watch(arguments: WatchArguments) -> ManagerResult<()> {
    let mut session = WatchSession::new(arguments)?;
    let shutdown = CancellationToken::new();
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let running = session.run(&shutdown);
    tokio::pin!(running);
    tokio::select! {
        outcome = &mut running => outcome,
        _ = async { tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} } } => {
            shutdown.cancel();
            running.await
        }
    }
}
impl WatchSession {
    fn new(arguments: WatchArguments) -> ManagerResult<Self> {
        let PluginTarget::Directory(package) = PluginTarget::parse(&arguments.test.target)? else {
            return Err("watchにはローカルの配布ディレクトリを指定してください".into());
        };
        let mut paths = if arguments.paths.is_empty() { vec![package.clone()] } else { arguments.paths };
        paths.push(arguments.test.pcap.clone());
        let paths = paths.iter().map(|path| expand_path(path)?.canonicalize().map_err(Into::into)).collect::<ManagerResult<Vec<_>>>()?;
        let mut excluded = arguments.excluded.iter().map(|path| absolute_directory(&expand_path(path)?)).collect::<ManagerResult<Vec<_>>>()?;
        if arguments.build.is_some() {
            if paths.iter().any(|path| path.starts_with(&package)) {
                return Err("ビルド時は配布ディレクトリの外にあるソースを--watchで指定してください".into());
            }
            excluded.push(package.clone());
        }
        let changes = Changes::start(paths, excluded)?;
        let timeout = Duration::from_secs(arguments.timeout_seconds);
        let build = arguments
            .build
            .map(|executable| {
                Ok::<_, Box<dyn std::error::Error>>(ProcessCommand {
                    executable: expand_path(&executable)?.into_os_string(),
                    arguments: arguments.build_arguments,
                    build: true,
                    timeout,
                })
            })
            .transpose()?;
        Ok(Self {
            test: test_command(&arguments.test, &package, timeout)?,
            build,
            changes,
            debounce: Duration::from_millis(arguments.debounce_ms),
            once: arguments.once,
        })
    }
    async fn run(&mut self, shutdown: &CancellationToken) -> ManagerResult<()> {
        let mut run = 0;
        loop {
            self.changes.begin_run()?;
            run += 1;
            eprintln!("[watch] run={run} status=started");
            let outcome = self.run_once(shutdown).await;
            if shutdown.is_cancelled() {
                return Ok(());
            }
            match &outcome {
                Ok(()) => eprintln!("[watch] run={run} status=passed"),
                Err(error) => eprintln!("[watch] run={run} status=failed: {error}"),
            }
            if self.once {
                return outcome;
            }
            eprintln!("[watch] run={run} status=waiting");
            if !self.changes.wait(self.debounce, shutdown).await? {
                return Ok(());
            }
        }
    }
    async fn run_once(&self, shutdown: &CancellationToken) -> ManagerResult<()> {
        if let Some(build) = &self.build {
            let status = process::run(build, shutdown).await?;
            if !status.success() {
                return Err(format!("ビルド失敗（{status}）。古い配布物のテストは実行しません").into());
            }
        }
        let status = process::run(&self.test, shutdown).await?;
        if !status.success() {
            return Err(format!("ブロック単体テスト失敗（{status}）").into());
        }
        Ok(())
    }
}
fn test_command(test: &BlockTestArguments, package: &Path, timeout: Duration) -> ManagerResult<ProcessCommand> {
    let mut arguments: Vec<OsString> = ["plugin", "block", "test"].into_iter().map(Into::into).collect();
    arguments.push(package.into());
    for (flag, value) in [
        ("--pcap", expand_path(&test.pcap)?.into_os_string()),
        ("--node-id", test.node_id.clone().into()),
        ("--channel", test.channel.clone().into()),
        ("--instance", test.instance.clone().into()),
    ] {
        arguments.extend([flag.into(), value]);
    }
    for assignment in &test.assignments {
        arguments.extend(["--set".into(), assignment.into()]);
    }
    if test.json {
        arguments.push("--json".into());
    }
    Ok(ProcessCommand {
        executable: std::env::current_exe()?.into(),
        arguments,
        build: false,
        timeout,
    })
}
