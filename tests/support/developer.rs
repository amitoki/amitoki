use amitoki::plugin_manager::Package;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[path = "package.rs"]
mod package_fixture;

pub struct Lab {
    pub root: tempfile::TempDir,
    pub package: PathBuf,
    pub store: PathBuf,
}
impl Lab {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("bundle");
        package_fixture::write_package(Path::new(env!("CARGO_BIN_EXE_amitoki-test-block")), &package);
        let store = root.path().join("plugins");
        Self { root, package, store }
    }
    pub fn command(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_amitoki"))
            .args(arguments)
            .current_dir(self.root.path())
            .env("AMITOKI_PLUGIN_DIR", &self.store)
            .env("HOME", self.root.path())
            .output()
            .unwrap()
    }
    pub fn success(&self, arguments: &[&str]) -> String {
        let output = self.command(arguments);
        assert!(
            output.status.success(),
            "args={arguments:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    pub fn failure(&self, arguments: &[&str]) -> String {
        let output = self.command(arguments);
        assert!(!output.status.success(), "{arguments:?}");
        String::from_utf8(output.stderr).unwrap()
    }
    pub fn installed(&self) -> Package {
        Package::load(&self.store.join("block-fixture")).unwrap()
    }
    pub fn save_package(&self, directory: &Path, package: &Package) {
        fs::write(directory.join("plugin.json"), serde_json::to_vec(package).unwrap()).unwrap();
    }
}

pub fn write_capture(path: &Path, packets: &[Vec<u8>]) {
    let mut bytes = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    for value in [0u32, 0, 65535, 1] {
        bytes.extend(value.to_le_bytes());
    }
    for (index, packet) in packets.iter().enumerate() {
        for value in [index as u32, 0, packet.len() as u32, packet.len() as u32] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(packet);
    }
    fs::write(path, bytes).unwrap();
}

pub fn reports(output: &str) -> Vec<Value> {
    output.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}
