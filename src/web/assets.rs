use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};

struct Asset {
    content_type: &'static str,
    bytes: &'static [u8],
}

// Viteのハッシュ付きファイルをビルド時に列挙し、実行時のファイル読み込みを不要にする。
include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

pub(super) async fn serve(uri: Uri) -> Response {
    let path = if uri.path() == "/" { "/index.html" } else { uri.path() };
    match lookup(path) {
        Some(asset) => ([(header::CONTENT_TYPE, asset.content_type)], asset.bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
