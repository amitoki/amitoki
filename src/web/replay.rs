//! 各PCAPは独立した再生プロセスで解析し、失敗時は前の結果を保持する。
use super::{State, MAX_PACKETS, MAX_REPORT_BYTES, REPLAY_TIMEOUT};
use crate::debug::PacketReport;
use std::{io::Write, process::Stdio};
use tokio::io::AsyncReadExt;

struct ReplayProcess {
    child: tokio::process::Child,
    group: i32,
}

impl Drop for ReplayProcess {
    fn drop(&mut self) {
        // タイムアウト・要求中断時も、実行中のStageを再生プロセスの子孫ごと止める。
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
    }
}

pub(super) struct CaptureInput<'a> {
    pub bytes: &'a [u8],
    pub name: &'a str,
    pub source: &'a str,
}

pub(super) async fn analyze(state: &State, input: CaptureInput<'_>) -> Result<bytes::Bytes, String> {
    let CaptureInput { bytes, name, source } = input;
    if source != "capture" && !state.topology.relays.iter().any(|relay| source == format!("{}.received", relay.id)) {
        return Err("再生元が不正です".into());
    }
    let mut capture = tempfile::NamedTempFile::new().map_err(|_| "PCAPを保存できません")?;
    capture.write_all(bytes).map_err(|_| "PCAPを保存できません")?;
    let child = tokio::process::Command::new(&state.executable)
        .args(["debug", "--directory"])
        .arg(&state.plugins)
        .args(["replay", "--config"])
        .arg(state.snapshot.path())
        .arg("--pcap")
        .arg(capture.path())
        .args([
            "--source",
            source,
            "--json",
            "--inspect",
            "--limit",
            &(MAX_PACKETS + 1).to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "解析プロセスを起動できません")?;
    let group = child.id().ok_or("解析プロセスを取得できません")? as i32;
    let mut process = ReplayProcess { child, group };
    let stdout = process.child.stdout.take().ok_or("解析結果を取得できません")?;
    let operation = async {
        let mut output = Vec::new();
        stdout.take(MAX_REPORT_BYTES as u64 + 1).read_to_end(&mut output).await.map_err(|_| "解析結果を読み込めません")?;
        if output.len() > MAX_REPORT_BYTES {
            return Err("解析結果が32MiBを超えました。小さいPCAPを指定してください".to_owned());
        }
        let status = process.child.wait().await.map_err(|_| "解析プロセスの終了を確認できません")?;
        let mut reports = Vec::new();
        for line in output.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            reports.push(serde_json::from_slice::<PacketReport>(line).map_err(|_| "解析結果が不正です")?);
        }
        // Stageの失敗は最後のreportに含まれる。PCAPの破損・初期化失敗は結果を置換しない。
        if !status.success() && !reports.last().is_some_and(|report| report.error.is_some()) {
            return Err("PCAPまたはStageの初期化に失敗しました。amitoki debug replayで詳細を確認してください".into());
        }
        let truncated = reports.len() as u64 > MAX_PACKETS;
        reports.truncate(MAX_PACKETS as usize);
        serde_json::to_vec(&serde_json::json!({"name":name,"source":source,"packets":reports,"truncated":truncated}))
            .map(bytes::Bytes::from)
            .map_err(|_| "解析結果を保存できません".into())
    };
    match tokio::time::timeout(REPLAY_TIMEOUT, operation).await {
        Ok(Ok(capture)) => Ok(capture),
        outcome => {
            unsafe {
                libc::kill(-process.group, libc::SIGKILL);
            }
            let _ = process.child.wait().await;
            match outcome {
                Ok(Err(error)) => Err(error),
                _ => Err("解析が120秒を超えました".into()),
            }
        },
    }
}
