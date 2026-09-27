use super::{replay, State as WebState, MAX_CAPTURE_BYTES};
use axum::{
    body::{to_bytes, Body},
    extract::{Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

// 遅いアップロードが解析スロットを占有し続けるのを防ぐ。
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
// 一般的なファイル名の上限に合わせ、表示名の保持量も制限する。
const MAX_CAPTURE_NAME_BYTES: usize = 255;

pub(super) fn router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", include_str!("../../web/index.html")) }))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/app.css", get(|| async { asset("text/css; charset=utf-8", include_str!("../../web/app.css")) }))
        .route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", include_str!("../../web/app.js")) }))
        .route("/views.js", get(|| async { asset("text/javascript; charset=utf-8", include_str!("../../web/views.js")) }))
        .route("/labels.js", get(|| async { asset("text/javascript; charset=utf-8", include_str!("../../web/labels.js")) }))
        .route(
            "/diagram.js",
            get(|| async { asset("text/javascript; charset=utf-8", include_str!("../../web/diagram.js")) }),
        )
        .route("/api/topology", get(topology))
        .route("/api/status", get(status))
        .route("/api/capture", get(capture).merge(post(upload)))
        .layer(middleware::from_fn_with_state(state.clone(), protect))
        .with_state(state)
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}

async fn protect(State(state): State<Arc<WebState>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let origin = format!("http://{}", state.authority);
    let permitted = headers.get(header::HOST).and_then(|value| value.to_str().ok()) == Some(&state.authority)
        && headers.get(header::ORIGIN).is_none_or(|value| value == origin.as_str())
        && headers.get("sec-fetch-site").is_none_or(|value| value != "cross-site");
    if !permitted {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.uri().path().starts_with("/api/") && headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()) != Some(&format!("Bearer {}", state.token)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut response = next.run(request).await;
    for (name, value) in [
        ("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"),
        ("x-content-type-options", "nosniff"), ("cache-control", "no-store"), ("referrer-policy", "no-referrer"),
    ] { response.headers_mut().insert(name, HeaderValue::from_static(value)); }
    response
}

async fn topology(State(state): State<Arc<WebState>>) -> Json<crate::observation::Topology> {
    Json(state.topology.clone())
}

async fn status(State(state): State<Arc<WebState>>) -> Response {
    match crate::control::status(&state.config).await {
        Ok(status) => Json(serde_json::json!({"running":true,"status":status})).into_response(),
        Err(_) => Json(serde_json::json!({"running":false,"status":null})).into_response(),
    }
}

async fn capture(State(state): State<Arc<WebState>>) -> Response {
    match state.capture.read() {
        Ok(capture) => ([(header::CONTENT_TYPE, "application/json")], Body::from(capture.clone())).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "解析結果を取得できません"),
    }
}

#[derive(Deserialize)]
struct Upload {
    name: String,
    #[serde(default = "capture_source")]
    source: String,
}
fn capture_source() -> String {
    "capture".into()
}

async fn upload(State(state): State<Arc<WebState>>, Query(query): Query<Upload>, request: Request) -> Response {
    let Ok(_permit) = state.replay_slot.try_acquire() else {
        return error(StatusCode::CONFLICT, "解析中です");
    };
    if query.name.len() > MAX_CAPTURE_NAME_BYTES || query.name.chars().any(char::is_control) {
        return error(StatusCode::BAD_REQUEST, "ファイル名が不正です");
    }
    if request.headers().get_all(header::CONTENT_TYPE).iter().count() != 1 || request.headers().get(header::CONTENT_TYPE).is_none_or(|value| value != "application/octet-stream") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "PCAPを指定してください");
    }
    let bytes = match tokio::time::timeout(UPLOAD_TIMEOUT, to_bytes(request.into_body(), MAX_CAPTURE_BYTES)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return error(StatusCode::PAYLOAD_TOO_LARGE, "PCAPは16MiB以内にしてください"),
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "PCAPの読み込みがタイムアウトしました"),
    };
    match replay::analyze(
        &state,
        replay::CaptureInput {
            bytes: &bytes,
            name: &query.name,
            source: &query.source,
        },
    )
    .await
    {
        Ok(capture) => {
            let Ok(mut current) = state.capture.write() else {
                return error(StatusCode::INTERNAL_SERVER_ERROR, "解析結果を保存できません");
            };
            *current = capture.clone();
            ([(header::CONTENT_TYPE, "application/json")], Body::from(capture)).into_response()
        },
        Err(message) => error(StatusCode::UNPROCESSABLE_ENTITY, &message),
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error":message}))).into_response()
}
