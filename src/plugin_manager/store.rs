use super::{package::read_options, ManagerResult, Package, PluginKind};
use amitoki_plugin_sdk::{
    block::{Block, BlockContext, BlockOutput, BlockPacket, ProcessBlock},
    ProcessRelay,
};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError};
use async_trait::async_trait;
use fs2::FileExt;
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub(crate) struct InstallRequest {
    pub update: bool,
    pub source: Option<String>,
    pub kind: Option<PluginKind>,
    pub name: Option<String>,
}

#[derive(Clone)]
pub struct PluginStore {
    pub directory: PathBuf,
}
impl PluginStore {
    pub fn from_environment() -> ManagerResult<Self> {
        let directory = if let Some(path) = std::env::var_os("AMITOKI_PLUGIN_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(path).join("amitoki/plugins")
        } else {
            PathBuf::from(std::env::var_os("HOME").ok_or("HOMEまたはAMITOKI_PLUGIN_DIRを設定してください")?).join(".local/share/amitoki/plugins")
        };
        Ok(Self { directory })
    }
    pub fn plugin_path(&self, name: &str) -> ManagerResult<PathBuf> {
        if name.is_empty() || name.len() > 64 || !name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return Err("プラグイン名が不正です".into());
        }
        Ok(self.directory.join(name))
    }
    pub fn list(&self) -> ManagerResult<Vec<Package>> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut packages = Vec::new();
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            packages.push(Package::load(&entry.path())?);
        }
        packages.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
        Ok(packages)
    }
    fn lock(&self, name: &str, exclusive: bool) -> ManagerResult<File> {
        self.plugin_path(name)?;
        std::fs::create_dir_all(self.directory.join(".locks"))?;
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.directory.join(".locks").join(name))?;
        let outcome = if exclusive {
            FileExt::try_lock_exclusive(&lock)
        } else {
            FileExt::try_lock_shared(&lock)
        };
        outcome.map_err(|_| "使用中または管理操作中のプラグインです。対象の中継を停止してから操作してください")?;
        Ok(lock)
    }
    pub fn install(&self, source: &Path, update: bool) -> ManagerResult<Package> {
        self.install_checked(
            source,
            InstallRequest {
                update,
                ..InstallRequest::default()
            },
        )
    }
    pub(crate) fn install_checked(&self, source: &Path, request: InstallRequest) -> ManagerResult<Package> {
        let mut package = Package::load(source)?;
        if let Some(kind) = request.kind {
            kind.check(&package)?;
        }
        if request.name.as_ref().is_some_and(|name| *name != package.manifest.name) {
            return Err("更新対象のプラグイン名が一致しません".into());
        }
        if let Some(source) = request.source {
            package.source = Some(source);
        }
        if package.manifest.name == "memory" {
            return Err("memoryは本体内のテスト用プラグインです".into());
        }
        package.verify(source)?;
        let _lock = self.lock(&package.manifest.name, true)?;
        let destination = self.plugin_path(&package.manifest.name)?;
        if destination.exists() {
            let installed = Package::load(&destination)?;
            PluginKind::of(&installed).check(&package)?;
        }
        if destination.exists() && !request.update {
            return Err("追加済みです。plugin updateを使ってください".into());
        }
        if self.directory.join(".config").join(format!("{}.json", package.manifest.name)).exists() {
            package.manifest.validate_options(&self.options(&package.manifest.name)?)?;
        }
        let staging = tempfile::Builder::new().prefix(".install-").tempdir_in(&self.directory)?;
        std::fs::copy(source.join(&package.binary), staging.path().join(&package.binary))?;
        std::fs::set_permissions(staging.path().join(&package.binary), std::fs::Permissions::from_mode(0o755))?;
        std::fs::write(staging.path().join("plugin.json"), serde_json::to_vec_pretty(&package)?)?;
        package.verify(staging.path())?;
        std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o755))?;
        if destination.exists() {
            exchange_directories(staging.path(), &destination)?;
        } else {
            std::fs::rename(staging.path(), &destination)?;
        }
        Ok(package)
    }
    pub fn remove(&self, name: &str) -> ManagerResult<()> {
        let _lock = self.lock(name, true)?;
        std::fs::remove_dir_all(self.plugin_path(name)?)?;
        // 接続先や鍵の参照を再入力せずに再インストールできるよう、設定は保持する。
        Ok(())
    }
    pub fn options(&self, name: &str) -> ManagerResult<Value> {
        self.plugin_path(name)?;
        read_options(&self.directory.join(".config").join(format!("{name}.json")))
    }
    pub fn save_options(&self, name: &str, options: &Value) -> ManagerResult<()> {
        let _lock = self.lock(name, true)?;
        Package::load(&self.plugin_path(name)?)?.manifest.validate_options(options)?;
        let directory = self.directory.join(".config");
        std::fs::create_dir_all(&directory)?;
        let temporary = tempfile::NamedTempFile::new_in(&directory)?;
        std::fs::write(temporary.path(), serde_json::to_vec_pretty(options)?)?;
        temporary.persist(directory.join(format!("{name}.json")))?;
        Ok(())
    }
    pub fn resolved_options(&self, name: &str, overrides: &Value) -> ManagerResult<Value> {
        let mut options = self.options(name)?;
        let values = options.as_object_mut().ok_or("保存された設定がオブジェクトではありません")?;
        values.extend(overrides.as_object().ok_or("optionsをテーブルで指定してください")?.clone());
        Package::load(&self.plugin_path(name)?)?.manifest.validate_options(&options)?;
        Ok(options)
    }
    pub async fn connect_block(&self, name: &str, context: BlockContext, options: Value) -> ManagerResult<Arc<dyn Block>> {
        Ok(self.connect_stage(name, context, options).await?.0)
    }
    pub async fn connect_stage(&self, name: &str, context: BlockContext, options: Value) -> ManagerResult<(Arc<dyn Block>, amitoki_plugin_sdk::PluginManifest)> {
        let store = self.clone();
        let name = name.to_owned();
        // バイナリのコピー・検証中も、本体のパケット処理を動かし続ける。
        let snapshot = tokio::task::spawn_blocking(move || store.snapshot_stage(&name, &options).map_err(|error| error.to_string())).await??;
        if snapshot.package.manifest.block.is_none() {
            return Err("Stageの定義がありません".into());
        }
        let block = ProcessBlock::connect(
            &snapshot.directory.path().join(&snapshot.package.binary),
            &snapshot.package.manifest,
            (context, snapshot.options),
        )
        .await?;
        Ok((
            Arc::new(InstalledBlock {
                block,
                _snapshot: snapshot.directory,
            }),
            snapshot.package.manifest,
        ))
    }
    fn snapshot_stage(&self, name: &str, options: &Value) -> ManagerResult<StageSnapshot> {
        // コピーが完了するまで更新を排他し、起動後はインストール先と寿命を分ける。
        let _lock = self.lock(name, false)?;
        let path = self.plugin_path(name)?;
        let package = Package::load(&path)?;
        PluginKind::Block.check(&package)?;
        let options = self.resolved_options(name, options)?;
        let directory = tempfile::Builder::new().prefix(".stage-").tempdir_in(&self.directory)?;
        std::fs::copy(path.join(&package.binary), directory.path().join(&package.binary))?;
        package.verify(directory.path())?;
        Ok(StageSnapshot { directory, package, options })
    }
    pub async fn connect(&self, name: &str, context: RelayContext, options: Value) -> ManagerResult<Arc<dyn Relay>> {
        let lock = self.lock(name, false)?;
        let path = self.plugin_path(name)?;
        let package = Package::load(&path)?;
        package.verify(&path)?;
        let options = self.resolved_options(name, &options)?;
        let relay = ProcessRelay::connect(&path.join(&package.binary), &package.manifest, (context, options)).await?;
        Ok(Arc::new(InstalledRelay { relay, _lock: lock }))
    }
}
struct InstalledRelay {
    relay: ProcessRelay,
    _lock: File,
}
#[async_trait]
impl Relay for InstalledRelay {
    async fn publish(&self, frames: &[Frame]) -> Result<(), RelayError> {
        self.relay.publish(frames).await
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        self.relay.receive(limit).await
    }
    async fn acknowledge(&self, receipts: &[Receipt]) -> Result<(), RelayError> {
        self.relay.acknowledge(receipts).await
    }
}

// Linuxの原子的な交換により、更新中の強制終了でも旧版か新版のどちらかが残る。
fn exchange_directories(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    let status = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_EXCHANGE) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

struct StageSnapshot {
    directory: tempfile::TempDir,
    package: Package,
    options: Value,
}

struct InstalledBlock {
    block: ProcessBlock,
    _snapshot: tempfile::TempDir,
}
#[async_trait]
impl Block for InstalledBlock {
    async fn generate(&self, request: &amitoki_plugin_sdk::packet::GenerateRequest) -> Result<Vec<Vec<u8>>, RelayError> {
        self.block.generate(request).await
    }
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        self.block.process(packets).await
    }
}
