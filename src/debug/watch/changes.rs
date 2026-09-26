//! 変更通知は1つの状態へまとめ、保存連打で無制限のキューを作らない。
use crate::plugin_manager::ManagerResult;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

// ソース監視でビルド出力を拾い、ビルドが自分自身を再起動する循環を避ける。
const IGNORED_DIRECTORIES: &[&str] = &[".git", "target", "dist", "artifacts", "node_modules", ".vm-lab", "__pycache__"];

#[derive(Clone, Default)]
struct ChangeState {
    error: Option<String>,
}

pub(super) struct Changes {
    _watcher: RecommendedWatcher,
    events: watch::Receiver<ChangeState>,
}
impl Changes {
    pub fn start(paths: Vec<PathBuf>, excluded: Vec<PathBuf>) -> ManagerResult<Self> {
        let (sender, events) = watch::channel(ChangeState::default());
        let roots: Vec<_> = paths.iter().map(|path| (path.clone(), path.is_dir())).collect();
        let directories: Vec<_> = roots.iter().filter(|(_, directory)| *directory).map(|(path, _)| path.clone()).collect();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| match event {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {},
            Ok(event) if event.need_rescan() || event.paths.iter().any(|path| relevant(path, &roots, &excluded)) => {
                sender.send_modify(|_| {});
            },
            Ok(_) => {},
            Err(error) => sender.send_modify(|state| state.error = Some(format!("変更監視に失敗しました: {error}"))),
        })?;
        for directory in &directories {
            if !directories.iter().any(|parent| parent != directory && directory.starts_with(parent)) {
                watcher.watch(directory, RecursiveMode::Recursive)?;
            }
        }
        let mut watched_parents = BTreeSet::new();
        for path in paths.iter().filter(|path| !path.is_dir()) {
            // 同じ親へのNonRecursive登録で、先の再帰監視を上書きしない。
            let parent = path.parent().ok_or("監視対象の親ディレクトリがありません")?;
            if !directories.iter().any(|directory| parent.starts_with(directory)) && watched_parents.insert(parent) {
                // ファイル自身でなく親を監視し、エディタによるrename保存にも追従する。
                watcher.watch(parent, RecursiveMode::NonRecursive)?;
            }
        }
        Ok(Self { _watcher: watcher, events })
    }

    pub fn begin_run(&mut self) -> ManagerResult<()> {
        check(&self.events.borrow_and_update())
    }

    pub async fn wait(&mut self, debounce: Duration, shutdown: &CancellationToken) -> ManagerResult<bool> {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(false),
            changed = self.events.changed() => changed?,
        }
        loop {
            check(&self.events.borrow_and_update())?;
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(false),
                changed = self.events.changed() => changed?,
                _ = tokio::time::sleep(debounce) => return Ok(true),
            }
        }
    }
}

fn check(state: &ChangeState) -> ManagerResult<()> {
    if let Some(error) = &state.error {
        return Err(error.clone().into());
    }
    Ok(())
}
fn relevant(path: &Path, roots: &[(PathBuf, bool)], excluded: &[PathBuf]) -> bool {
    if excluded.iter().any(|excluded| path.starts_with(excluded)) {
        return false;
    }
    roots.iter().any(|(root, directory)| {
        if !directory {
            return path == root;
        }
        path.strip_prefix(root).is_ok_and(|relative| !relative.components().any(|component| IGNORED_DIRECTORIES.iter().any(|ignored| component.as_os_str() == *ignored)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_changes_are_kept_but_generated_files_are_ignored() {
        let roots = vec![
            (PathBuf::from("/project"), true),
            (PathBuf::from("/captures/input.pcap"), false),
        ];
        let excluded = vec![PathBuf::from("/project/package")];
        for path in ["/project/src/main.rs", "/captures/input.pcap"] {
            assert!(relevant(Path::new(path), &roots, &excluded));
        }
        for path in [
            "/project/target/build",
            "/project/.git/index",
            "/project/package/plugin.json",
            "/captures/other.pcap",
            "/other/src.rs",
        ] {
            assert!(!relevant(Path::new(path), &roots, &excluded));
        }
    }
}
