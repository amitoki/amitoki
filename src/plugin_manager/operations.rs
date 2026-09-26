use super::{
    download::download,
    source::{absolute_directory, expand_path, GithubSource, PluginTarget},
    store::InstallRequest,
    ManagerResult, Package, PluginKind, PluginStore,
};
use std::path::PathBuf;

pub(super) struct InstallOptions {
    pub kind: Option<PluginKind>,
    pub target: Option<String>,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

impl InstallOptions {
    fn target(&self, legacy_add: bool) -> ManagerResult<PluginTarget> {
        if let Some(path) = &self.path {
            return Ok(PluginTarget::Directory(absolute_directory(&expand_path(path)?)?));
        }
        let target = self.target.as_deref().ok_or("名前、GitHub URL、ディレクトリを指定してください")?;
        if legacy_add && self.kind.is_none() && !target.contains("://") && !target.starts_with(['/', '.', '~']) {
            let (name, version) = target.split_once('@').map_or((target, None), |(name, version)| (name, Some(version)));
            let repository = match name {
                "postgres" => "amitoki/amitoki-plugin-postgres",
                "p2p" => "amitoki/amitoki-plugin-p2p",
                other => other,
            };
            if repository.contains('/') {
                return Ok(PluginTarget::Github(GithubSource::parse(repository)?.with_version(version)?));
            }
        }
        PluginTarget::parse(target)
    }
}

pub(super) async fn add(store: &PluginStore, options: InstallOptions) -> ManagerResult<()> {
    let target = options.target(true)?;
    if let PluginTarget::Name(_) = target {
        if options.version.is_some() {
            return Err("追加済みの版を変える場合はupdateを使ってください".into());
        }
        let package = target.installed(store, options.kind)?;
        println!("{} {}は追加済みです", package.manifest.name, package.manifest.version);
        return Ok(());
    }
    let request = InstallRequest {
        kind: options.kind,
        ..InstallRequest::default()
    };
    let package = install(store, target, (request, options.version)).await?;
    println!("{} {}を追加しました", package.manifest.name, package.manifest.version);
    Ok(())
}

pub(super) async fn update(store: &PluginStore, options: InstallOptions) -> ManagerResult<()> {
    let requested = PluginTarget::parse(options.target.as_deref().ok_or("更新対象を指定してください")?)?;
    let installed = requested.installed(store, options.kind)?;
    let source = if options.path.is_some() {
        options.target(false)?
    } else if let PluginTarget::Name(_) = requested {
        let recorded = installed.source.as_deref().ok_or("取得元が未記録です。--pathで配布ディレクトリを指定してください")?;
        if recorded.starts_with('/') {
            PluginTarget::Directory(PathBuf::from(recorded))
        } else {
            let mut source = GithubSource::parse(recorded)?;
            // 明示した版への更新だけが、保存済みの版固定を変更する。
            if options.version.is_some() {
                source.version = None;
            }
            PluginTarget::Github(source)
        }
    } else if let PluginTarget::Github(mut source) = requested {
        if source.version.is_none() && options.version.is_none() {
            source.version = installed.source.as_deref().and_then(|recorded| GithubSource::parse(recorded).ok()).and_then(|recorded| recorded.version);
        }
        PluginTarget::Github(source)
    } else {
        requested
    };
    let request = InstallRequest {
        update: true,
        kind: Some(PluginKind::of(&installed)),
        name: Some(installed.manifest.name.clone()),
        source: None,
    };
    let package = install(store, source, (request, options.version)).await?;
    println!("{} {}へ更新しました", package.manifest.name, package.manifest.version);
    Ok(())
}

async fn install(store: &PluginStore, target: PluginTarget, options: (InstallRequest, Option<String>)) -> ManagerResult<Package> {
    let (mut request, version) = options;
    match target {
        PluginTarget::Github(source) => {
            let source = source.with_version(version.as_deref())?;
            let directory = download(&source).await?;
            request.source = Some(source.recorded());
            store.install_checked(directory.path(), request)
        },
        PluginTarget::Directory(path) => {
            if version.is_some() {
                return Err("--versionはGitHubから取得する場合に指定してください".into());
            }
            request.source = Some(path.to_str().ok_or("取得元のパスはUTF-8で指定してください")?.to_owned());
            store.install_checked(&path, request)
        },
        PluginTarget::Name(_) => Err("取得元を指定してください".into()),
    }
}
