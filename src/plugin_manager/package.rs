use super::ManagerResult;
use amitoki_plugin_sdk::PluginManifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

// 配布物の破損や誤ったファイル指定で巨大なメモリ確保を行わない。
pub const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub manifest: PluginManifest,
    pub target: String,
    pub binary: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub fn target() -> String {
    format!("{}-unknown-linux-gnu", std::env::consts::ARCH)
}

impl Package {
    pub fn load(directory: &Path) -> ManagerResult<Self> {
        let path = directory.join("plugin.json");
        if path.metadata()?.len() > MAX_MANIFEST_BYTES {
            return Err("プラグイン定義が大きすぎます".into());
        }
        let package: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        package.manifest.validate()?;
        if package.target != target() {
            return Err("このOS・CPU用のプラグインではありません".into());
        }
        if package.binary != format!("amitoki-plugin-{}", package.manifest.name) {
            return Err("実行ファイル名が不正です".into());
        }
        Ok(package)
    }
    pub fn verify(&self, directory: &Path) -> ManagerResult<()> {
        let path = directory.join(&self.binary);
        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_BINARY_BYTES {
            return Err("実行ファイルの種類またはサイズが不正です".into());
        }
        let mut file = File::open(path)?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut hash)?;
        if format!("{:x}", hash.finalize()) != self.sha256 {
            return Err("実行ファイルのSHA256が一致しません".into());
        }
        Ok(())
    }
}

pub fn read_options(path: &Path) -> ManagerResult<serde_json::Value> {
    match File::open(path) {
        Ok(file) => {
            let mut contents = String::new();
            file.take(MAX_MANIFEST_BYTES + 1).read_to_string(&mut contents)?;
            if contents.len() as u64 > MAX_MANIFEST_BYTES {
                return Err("設定が大きすぎます".into());
            }
            serde_json::from_str(&contents).map_err(|_| "保存されたプラグイン設定を読み込めません".into())
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(error.into()),
    }
}
