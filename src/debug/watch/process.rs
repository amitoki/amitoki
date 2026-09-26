//! ビルド・単体試験を直列実行し、停止時は子孫プロセスも終了させる。
use crate::plugin_manager::ManagerResult;
use std::{
    ffi::OsString,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

pub(super) struct ProcessCommand {
    pub executable: OsString,
    pub arguments: Vec<OsString>,
    pub build: bool,
    pub timeout: Duration,
}
struct ProcessGroup {
    child: Child,
    id: i32,
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // ビルドスクリプトの孫プロセスを、次の実行やwatch終了後に残さない。
        unsafe {
            libc::kill(-self.id, libc::SIGKILL);
        }
    }
}

pub(super) async fn run(command: &ProcessCommand, shutdown: &CancellationToken) -> ManagerResult<ExitStatus> {
    let mut process = Command::new(&command.executable);
    process.args(&command.arguments).stdin(Stdio::null()).stderr(Stdio::inherit()).process_group(0).kill_on_drop(true);
    // JSONLのテスト結果と、ビルドツールのログを混ぜない。
    if command.build {
        let stderr = std::fs::OpenOptions::new().write(true).open("/dev/stderr")?;
        process.stdout(Stdio::from(stderr));
    } else {
        process.stdout(Stdio::inherit());
    }
    let child = process.spawn()?;
    let id = child.id().ok_or("子プロセスのIDを取得できません")? as i32;
    let mut group = ProcessGroup { child, id };
    let outcome = tokio::select! {
        status = group.child.wait() => Ok(status?),
        _ = shutdown.cancelled() => Err("watchを停止しました".into()),
        _ = tokio::time::sleep(command.timeout) => Err("ビルドまたはテストがタイムアウトしました".into()),
    };
    unsafe {
        libc::kill(-group.id, libc::SIGKILL);
    }
    group.child.wait().await?;
    outcome
}
