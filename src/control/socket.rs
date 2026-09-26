use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::net::UnixListener;

pub(super) struct ControlSocket {
    pub listener: UnixListener,
    path: PathBuf,
    _lock: File,
}

pub(super) fn path(config: &Path) -> io::Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    let directory = PathBuf::from(format!("/tmp/amitoki-control-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {},
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "制御ソケット用ディレクトリの所有者・権限が不正です"));
    }
    use std::os::unix::ffi::OsStrExt;
    let digest = Sha256::digest(config.canonicalize()?.as_os_str().as_bytes());
    Ok(directory.join(format!("{:x}.sock", digest)))
}

use std::os::unix::fs::DirBuilderExt;

impl ControlSocket {
    pub fn bind(config: &Path) -> io::Result<Self> {
        let path = path(config)?;
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path.with_extension("lock"))?;
        FileExt::try_lock_exclusive(&lock).map_err(|_| io::Error::new(io::ErrorKind::AddrInUse, "同じ設定のamitokiが稼働中です"))?;
        // 排他ロック取得後だけ、前回異常終了で残ったソケットを取り除く。
        match std::fs::remove_file(&path) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self { listener, path, _lock: lock })
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
