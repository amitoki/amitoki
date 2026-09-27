//! 実HTTP・実再生プロセスを使い、UIと外部Stageの境界を確認する。
#[allow(dead_code)]
#[path = "support/developer.rs"]
mod support;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
};

struct Server {
    _lab: support::Lab,
    child: Child,
    url: String,
    token: String,
    capture: Vec<u8>,
}
impl Server {
    async fn start() -> Self {
        let lab = support::Lab::new();
        lab.success(&["plugin", "stage", "add", "./bundle"]);
        std::fs::write(lab.root.path().join("app.toml"), "node_id='web-test'\nchannel='test'\ninterface='unused'\n[firewall]\npolicy='blacklist'\n[pipeline]\nroutes=[{from='capture',to=['first']},{from='first.pass',to=['second']},{from='second.pass',to=['sink']},{from='sink.received',to=['inject']}]\n[[pipeline.stages]]\nid='first'\nplugin='block-fixture'\n[[pipeline.stages]]\nid='second'\nplugin='block-fixture'\n[[pipeline.relays]]\nid='sink'\nplugin='memory'\n").unwrap();
        support::write_capture(&lab.root.path().join("input.pcap"), &[vec![1; 14], vec![2; 12]]);
        let capture = std::fs::read(lab.root.path().join("input.pcap")).unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_amitoki"))
            .args(["web", "--config", "app.toml", "--listen", "127.0.0.1:0"])
            .env("AMITOKI_PLUGIN_DIR", &lab.store)
            .current_dir(lab.root.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), BufReader::new(child.stdout.take().unwrap()).read_line(&mut line)).await.unwrap().unwrap();
        let (url, token) = line.trim().split_once("/#").expect("Webの起動URL");
        Self {
            _lab: lab,
            child,
            url: url.into(),
            token: token.into(),
            capture,
        }
    }
    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        Client::new().get(format!("{}{path}", self.url)).bearer_auth(&self.token)
    }
    fn upload(&self, bytes: Vec<u8>) -> reqwest::RequestBuilder {
        Client::new().post(format!("{}/api/capture?name=input.pcap", self.url)).bearer_auth(&self.token).header("content-type", "application/octet-stream").body(bytes)
    }
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
    }
}

#[tokio::test]
async fn local_api_requires_session_token_and_rejects_foreign_hosts_and_origins() {
    let server = Server::start().await;
    assert_eq!(
        Client::new().get(format!("{}/api/topology", server.url)).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server.get("/api/topology").header("host", "attacker.example").send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        server.get("/api/topology").header("origin", "https://attacker.example").send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let page = server.get("/").send().await.unwrap();
    assert!(page.headers()["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    assert_eq!(page.headers()["cache-control"], "no-store");
    let html = page.text().await.unwrap();
    assert!(!html.contains("/src/main.tsx"));
    let assets: Vec<_> = html.split('"').filter(|part| part.starts_with("/assets/")).collect();
    assert!(assets.iter().any(|path| path.ends_with(".js")));
    assert!(assets.iter().any(|path| path.ends_with(".css")));
    for path in assets {
        let response = server.get(path).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = if path.ends_with(".js") { "text/javascript" } else { "text/css" };
        assert!(response.headers()["content-type"].to_str().unwrap().starts_with(content_type));
        assert!(!response.bytes().await.unwrap().is_empty());
    }
    assert_eq!(server.get("/src/main.tsx").send().await.unwrap().status(), StatusCode::NOT_FOUND);
    assert_eq!(server.get("/assets/missing.js").send().await.unwrap().status(), StatusCode::NOT_FOUND);
    let topology: Value = server.get("/api/topology").send().await.unwrap().json().await.unwrap();
    assert_eq!(topology["stages"][0]["id"], "first");
    assert!(topology["stages"][0].get("options").is_none());
    let status: Value = server.get("/api/status").send().await.unwrap().json().await.unwrap();
    assert_eq!(status["running"], false);
    server.stop().await;
}

#[tokio::test]
async fn upload_shows_real_stage_inputs_outputs_and_keeps_previous_capture_on_failure() {
    let server = Server::start().await;
    let response = server.upload(server.capture.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let capture: Value = response.json().await.unwrap();
    let first = &capture["packets"][0];
    assert_eq!(first["steps"][1]["input"]["annotations"]["instance"], "first");
    assert_eq!(first["steps"][1]["output"]["annotations"]["instance"], "second");
    assert_eq!(first["steps"][0]["input"]["length"], 14);
    assert!(capture["packets"][1]["rejection"].is_string());
    assert_eq!(server.upload(b"not a pcap".to_vec()).send().await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(server.get("/api/capture").send().await.unwrap().json::<Value>().await.unwrap(), capture);
    // 起動後の未適用設定で、再生内容がひそかに変わらない。
    std::fs::write(server._lab.root.path().join("app.toml"), "broken").unwrap();
    assert_eq!(server.upload(server.capture.clone()).send().await.unwrap().status(), StatusCode::OK);
    server.stop().await;
}

#[tokio::test]
async fn replay_source_and_body_limits_are_enforced() {
    let server = Server::start().await;
    let received: Value = Client::new()
        .post(format!("{}/api/capture?name=input.pcap&source=sink.received", server.url))
        .bearer_auth(&server.token)
        .header("content-type", "application/octet-stream")
        .body(server.capture.clone())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(received["packets"][0]["terminals"], serde_json::json!(["inject"]));
    assert_eq!(server.upload(vec![0; 16 * 1024 * 1024 + 1]).send().await.unwrap().status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        server.upload(server.capture.clone()).header("content-type", "text/plain").send().await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    server.stop().await;
}
