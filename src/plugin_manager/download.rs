use super::{
    package::{target, MAX_BINARY_BYTES, MAX_MANIFEST_BYTES},
    ManagerResult, Package,
};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION},
    Client,
};
use serde::Deserialize;
use std::{io::Write, time::Duration};

// GitHubへの到達不能時にCLIを待たせ続けない。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    url: String,
    size: u64,
}

pub async fn download(source: &str) -> ManagerResult<tempfile::TempDir> {
    let (repository, version) = source.split_once('@').map_or((source, None), |(repository, version)| (repository, Some(version)));
    let repository = match repository {
        "postgres" => "amitoki/amitoki-plugin-postgres",
        "p2p" => "amitoki/amitoki-plugin-p2p",
        other => other,
    };
    let segments: Vec<_> = repository.split('/').collect();
    if segments.len() != 2 || segments.iter().any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))) {
        return Err("取得先をowner/repository[@version]で指定してください".into());
    }
    if version.is_some_and(|tag| tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))) {
        return Err("リリースタグが不正です".into());
    }
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));
    if let Ok(token) = std::env::var("AMITOKI_GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| "GitHubトークンの形式が不正です")?;
        authorization.set_sensitive(true);
        headers.insert(AUTHORIZATION, authorization);
    }
    let client = Client::builder().user_agent("amitoki-plugin-manager").default_headers(headers).timeout(DOWNLOAD_TIMEOUT).build()?;
    let route = version.map_or("latest".to_owned(), |version| format!("tags/{version}"));
    let response = client.get(format!("https://api.github.com/repos/{repository}/releases/{route}")).send().await.map_err(|_| "GitHubへ接続できません")?;
    if !response.status().is_success() {
        return Err(format!(
            "リリースを取得できません (HTTP {})。privateリポジトリにはAMITOKI_GITHUB_TOKENを設定してください",
            response.status().as_u16()
        )
        .into());
    }
    let release: Release = response.json().await.map_err(|_| "GitHubのリリース情報が不正です")?;
    let directory = tempfile::tempdir()?;
    let manifest_name = format!("plugin-{}.json", target());
    let manifest_asset = release.assets.iter().find(|asset| asset.name == manifest_name).ok_or("このOS・CPU用のリリースがありません")?;
    download_asset(&client, manifest_asset, (&directory.path().join("plugin.json"), MAX_MANIFEST_BYTES)).await?;
    let mut package = Package::load(directory.path())?;
    if release.tag_name != format!("v{}", package.manifest.version) {
        return Err("リリースタグとプラグインのバージョンが一致しません".into());
    }
    let binary_name = format!("{}-{}", package.binary, target());
    let binary = release.assets.iter().find(|asset| asset.name == binary_name).ok_or("リリースに実行ファイルがありません")?;
    download_asset(&client, binary, (&directory.path().join(&package.binary), MAX_BINARY_BYTES)).await?;
    package.verify(directory.path())?;
    package.source = Some(repository.to_owned());
    std::fs::write(directory.path().join("plugin.json"), serde_json::to_vec_pretty(&package)?)?;
    Ok(directory)
}

async fn download_asset(client: &Client, asset: &Asset, destination: (&std::path::Path, u64)) -> ManagerResult<()> {
    let (path, limit) = destination;
    if asset.size > limit || !asset.url.starts_with("https://api.github.com/repos/") {
        return Err("リリースファイルのサイズまたは取得先が不正です".into());
    }
    let mut response = client
        .get(&asset.url)
        .header(ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|_| "リリースファイルを取得できません")?
        .error_for_status()
        .map_err(|_| "リリースファイルの取得に失敗しました")?;
    let mut file = std::fs::File::create(path)?;
    let mut received = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| "リリースファイルの取得が中断されました")? {
        received += chunk.len() as u64;
        if received > limit {
            return Err("リリースファイルがサイズ上限を超えました".into());
        }
        file.write_all(&chunk)?;
    }
    if received != asset.size {
        return Err("リリースファイルのサイズが一致しません".into());
    }
    Ok(())
}
