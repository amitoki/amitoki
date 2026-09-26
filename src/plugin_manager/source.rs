//! 取得元と追加済みの名前を区別する。管理操作の解決では通信しない。
use super::{ManagerResult, Package, PluginKind, PluginStore};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubSource {
    pub repository: String,
    pub version: Option<String>,
}

impl GithubSource {
    pub fn parse(value: &str) -> ManagerResult<Self> {
        let (repository, version) = if value.starts_with("https://") {
            let url = reqwest::Url::parse(value)?;
            if url.host_str() != Some("github.com")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("GitHubリポジトリのHTTPS URLを指定してください".into());
            }
            let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
            let version = match parts.as_slice() {
                [_, _] => None,
                [_, _, "releases", "tag", version] => Some((*version).to_owned()),
                _ => return Err("GitHubのリポジトリURLまたはReleaseのタグURLを指定してください".into()),
            };
            (format!("{}/{}", parts[0], parts[1].strip_suffix(".git").unwrap_or(parts[1])), version)
        } else {
            let (repository, version) = value.split_once('@').map_or((value, None), |(repository, version)| (repository, Some(version.to_owned())));
            (repository.to_owned(), version)
        };
        let parts: Vec<_> = repository.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|part| !valid_part(part)) || version.as_deref().is_some_and(|version| !valid_part(version)) {
            return Err("GitHubのリポジトリ名またはリリースタグが不正です".into());
        }
        Ok(Self {
            repository: repository.to_ascii_lowercase(),
            version,
        })
    }

    pub fn with_version(mut self, version: Option<&str>) -> ManagerResult<Self> {
        if let Some(version) = version {
            if !valid_part(version) || self.version.as_deref().is_some_and(|previous| previous != version) {
                return Err("リリースタグが不正、またはURLのタグと--versionが一致しません".into());
            }
            self.version = Some(version.to_owned());
        }
        Ok(self)
    }

    pub fn recorded(&self) -> String {
        self.version.as_ref().map_or_else(|| self.repository.clone(), |version| format!("{}@{version}", self.repository))
    }
}

fn valid_part(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

#[derive(Debug)]
pub enum PluginTarget {
    Name(String),
    Github(GithubSource),
    Directory(PathBuf),
}

impl PluginTarget {
    pub fn parse(value: &str) -> ManagerResult<Self> {
        if value.contains("://") || value.starts_with("git@") {
            if !value.starts_with("https://") {
                return Err("GitHubのHTTPS URLを指定してください".into());
            }
            return Ok(Self::Github(GithubSource::parse(value)?));
        }
        if value.starts_with(['/', '.', '~']) || value.contains('/') {
            return Ok(Self::Directory(absolute_directory(&expand_path(Path::new(value))?)?));
        }
        Ok(Self::Name(value.to_owned()))
    }

    pub fn installed(&self, store: &PluginStore, kind: Option<PluginKind>) -> ManagerResult<Package> {
        let package = match self {
            Self::Name(name) => Package::load(&store.plugin_path(name)?)?,
            _ => {
                let candidates: Vec<_> = store.list()?.into_iter().filter(|package| kind.is_none_or(|kind| PluginKind::of(package) == kind) && self.matches(package)).collect();
                match candidates.len() {
                    0 => return Err("この取得元のプラグインは未追加です。plugin TYPE addで追加してください".into()),
                    1 => candidates.into_iter().next().unwrap(),
                    _ => return Err("この取得元に複数のプラグインがあります。追加済みの名前を指定してください".into()),
                }
            },
        };
        if let Some(kind) = kind {
            kind.check(&package)?;
        }
        Ok(package)
    }

    fn matches(&self, package: &Package) -> bool {
        let Some(source) = &package.source else {
            return false;
        };
        match self {
            Self::Github(requested) => GithubSource::parse(source).is_ok_and(|installed| installed.repository == requested.repository),
            Self::Directory(requested) => Path::new(source).is_absolute() && Path::new(source) == requested,
            Self::Name(name) => package.manifest.name == *name,
        }
    }
}

pub fn expand_path(path: &Path) -> ManagerResult<PathBuf> {
    if path == Path::new("~") || path.starts_with("~/") {
        let home = std::env::var_os("HOME").ok_or("~の展開にはHOMEを設定してください")?;
        return Ok(PathBuf::from(home).join(path.strip_prefix("~")?));
    }
    if path.to_string_lossy().starts_with('~') {
        return Err("~または~/から始まるパスを指定してください（~userは非対応）".into());
    }
    Ok(path.to_owned())
}

pub fn absolute_directory(path: &Path) -> ManagerResult<PathBuf> {
    let path = if path.is_absolute() { path.to_owned() } else { std::env::current_dir()?.join(path) };
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    // 開発ディレクトリを削除した後も、保存済みの取得元で登録を削除できる。
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {},
            Component::ParentDir => {
                normalized.pop();
            },
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_and_release_urls_resolve_to_the_same_identity() {
        for url in ["https://github.com/Amitoki/Example", "https://github.com/amitoki/example.git/"] {
            assert_eq!(
                GithubSource::parse(url).unwrap(),
                GithubSource {
                    repository: "amitoki/example".into(),
                    version: None
                }
            );
        }
        let release = GithubSource::parse("https://github.com/amitoki/example/releases/tag/v0.1.0").unwrap();
        assert_eq!(release.recorded(), "amitoki/example@v0.1.0");
        assert!(release.with_version(Some("v0.2.0")).is_err());
    }

    #[test]
    fn foreign_hosts_credentials_and_unsupported_github_paths_are_rejected() {
        for url in [
            "http://github.com/a/b",
            "https://example.com/a/b",
            "https://github.com.evil.test/a/b",
            "https://user@github.com/a/b",
            "https://github.com/a/b/tree/main",
            "https://github.com/a/b?token=x",
            "https://github.com/a/b#readme",
            "git@github.com:a/b",
            "https://github.com/a/b/releases/tag/v1%2Ftest",
        ] {
            assert!(PluginTarget::parse(url).is_err(), "{url}");
        }
    }

    #[test]
    fn bare_names_are_not_guessed_from_a_working_directory() {
        assert!(matches!(PluginTarget::parse("example").unwrap(), PluginTarget::Name(_)));
        for path in ["./example", "../example", "/tmp/example", "dist/example"] {
            assert!(matches!(PluginTarget::parse(path).unwrap(), PluginTarget::Directory(_)));
        }
    }
}
