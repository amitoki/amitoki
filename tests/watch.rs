#[path = "support/package.rs"]
mod package;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

// 遅いCIでも、実行通知を待ってからファイルを書き換える。
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);
struct WatchLab {
    root: tempfile::TempDir,
}
impl WatchLab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        package::write_package(Path::new(env!("CARGO_BIN_EXE_amitoki-test-block")), &root.path().join("bundle"));
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/source"), "initial").unwrap();
        let mut capture = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
        for value in [0u32, 0, 65535, 1, 0, 0, 14, 14] {
            capture.extend(value.to_le_bytes());
        }
        capture.extend([1u8; 14]);
        fs::write(root.path().join("input.pcap"), capture).unwrap();
        Self { root }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_amitoki"));
        command.current_dir(self.root.path()).env("AMITOKI_PLUGIN_DIR", self.root.path().join("store")).args([
            "plugin",
            "block",
            "watch",
            "./bundle",
            "--pcap",
            "input.pcap",
            "--json",
            "--debounce-ms",
            "20",
        ]);
        command
    }
    fn write(&self, path: &str, contents: &str) {
        fs::write(self.root.path().join(path), contents).unwrap();
    }
    fn build_command(&self, script: &str) -> Command {
        self.write("build.sh", script);
        let mut command = self.command();
        command.args(["--watch", "src", "--build", "/bin/sh", "--build-arg", "build.sh"]);
        command
    }
}

struct RunningWatch {
    child: Child,
    stderr: Receiver<String>,
}
impl RunningWatch {
    fn start(mut command: Command) -> Self {
        let mut child = command.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let reader = BufReader::new(child.stderr.take().unwrap());
        let (sender, stderr) = mpsc::channel();
        std::thread::spawn(move || {
            for line in reader.lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, stderr }
    }
    fn wait_for(&self, expected: &str) {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        let mut seen = Vec::new();
        loop {
            let line = self.stderr.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or_else(|error| panic!("waiting for {expected}: {error}; {seen:?}"));
            if line.contains(expected) {
                return;
            }
            seen.push(line);
        }
    }
    fn stop(&mut self) {
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for RunningWatch {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
            self.child.wait().unwrap();
        }
    }
}

#[test]
fn once_keeps_build_logs_out_of_json_and_tests_a_local_copy() {
    let lab = WatchLab::new();
    fs::rename(lab.root.path().join("bundle"), lab.root.path().join("seed")).unwrap();
    let log = lab.root.path().join("build.log");
    let output = lab.build_command("cp -R seed bundle\necho build-output\n").arg("--once").stderr(fs::File::create(&log).unwrap()).output().unwrap();
    let log = fs::read_to_string(log).unwrap();
    assert!(output.status.success(), "{log}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["terminals"][0], "output:pass");
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["[watch] run=1 status=started", "build-output", "[watch] run=1 status=passed"]
    );
    assert!(!lab.root.path().join("store").exists());
}

#[test]
fn failed_build_does_not_test_an_old_package() {
    let lab = WatchLab::new();
    let output = lab.build_command("exit 3\n").arg("--once").output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("古い配布物"));
}

#[test]
fn watch_recovers_from_failure_and_retains_changes_during_a_build() {
    let lab = WatchLab::new();
    let command = lab.build_command("echo build >> builds\nif [ -f fail ]; then exit 3; fi\nif [ -p gate ]; then echo gate-ready >&2; read -r proceed < gate; fi\n");
    let mut watch = RunningWatch::start(command);
    watch.wait_for("run=1 status=waiting");
    lab.write("fail", "");
    lab.write("src/source", "bad");
    watch.wait_for("run=2 status=failed");
    watch.wait_for("run=2 status=waiting");
    fs::remove_file(lab.root.path().join("fail")).unwrap();
    assert!(Command::new("mkfifo").arg(lab.root.path().join("gate")).status().unwrap().success());
    // rename保存も拾う。gateでビルドを待たせ、実行中に次の変更を届ける。
    lab.write("replacement", "good");
    fs::rename(lab.root.path().join("replacement"), lab.root.path().join("src/source")).unwrap();
    watch.wait_for("gate-ready");
    lab.write("src/source", "newer");
    lab.write("gate", "continue\n");
    fs::remove_file(lab.root.path().join("gate")).unwrap();
    watch.wait_for("run=3 status=passed");
    watch.wait_for("run=4 status=waiting");
    watch.stop();
    assert_eq!(fs::read_to_string(lab.root.path().join("builds")).unwrap().lines().count(), 4);
}

#[test]
fn watching_a_capture_alongside_its_parent_keeps_recursive_source_events() {
    let lab = WatchLab::new();
    let mut command = lab.command();
    command.args(["--watch", "."]);
    let mut watch = RunningWatch::start(command);
    watch.wait_for("run=1 status=waiting");
    lab.write("src/source", "changed");
    watch.wait_for("run=2 status=waiting");
    fs::copy(lab.root.path().join("input.pcap"), lab.root.path().join("replacement")).unwrap();
    fs::rename(lab.root.path().join("replacement"), lab.root.path().join("input.pcap")).unwrap();
    watch.wait_for("run=3 status=waiting");
    watch.stop();
}

#[test]
fn timed_out_builds_stop_their_descendants_and_skip_the_test() {
    let lab = WatchLab::new();
    let output = lab.build_command("sleep 30 &\necho $! > descendant\nwait\n").args(["--once", "--timeout-seconds", "1"]).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("タイムアウト"));
    let pid = fs::read_to_string(lab.root.path().join("descendant")).unwrap();
    // 親が終了した直後はinitによる回収前のzombieが残る場合がある。
    if let Ok(status) = fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
        assert_eq!(status.split(')').nth(1).unwrap().trim().chars().next(), Some('Z'));
    }
}

#[test]
fn termination_stops_a_running_build_before_starting_the_test() {
    let lab = WatchLab::new();
    let mut watch = RunningWatch::start(lab.build_command("sleep 30 &\necho $! > descendant\necho signal-ready >&2\nwait\n"));
    watch.wait_for("signal-ready");
    watch.stop();
    let pid = fs::read_to_string(lab.root.path().join("descendant")).unwrap();
    if let Ok(status) = fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
        assert_eq!(status.split(')').nth(1).unwrap().trim().chars().next(), Some('Z'));
    }
}
