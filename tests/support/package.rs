use amitoki::plugin_manager::Package;
use amitoki_plugin_sdk::PluginManifest;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

pub fn write_package(executable: &Path, destination: &Path) -> Package {
    fs::create_dir_all(destination).unwrap();
    let description = Command::new(executable).arg("--describe").output().unwrap();
    assert!(description.status.success());
    let manifest: PluginManifest = serde_json::from_slice(&description.stdout).unwrap();
    let bytes = fs::read(executable).unwrap();
    let package = Package {
        binary: format!("amitoki-plugin-{}", manifest.name),
        manifest,
        target: format!("{}-unknown-linux-gnu", std::env::consts::ARCH),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        source: None,
    };
    fs::write(destination.join(&package.binary), bytes).unwrap();
    fs::write(destination.join("plugin.json"), serde_json::to_vec(&package).unwrap()).unwrap();
    package
}
