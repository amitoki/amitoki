#[path = "support/developer.rs"]
mod developer;
use developer::{reports, write_capture, Lab};
use serde_json::json;
use std::fs;

#[test]
fn local_paths_and_installed_names_manage_the_same_copy_without_changing_the_source() {
    let lab = Lab::new();
    lab.success(&["plugin", "block", "add", "./bundle"]);
    assert_eq!(lab.installed().source.as_deref(), lab.package.to_str());
    assert!(lab.success(&["plugin", "block", "add", "block-fixture"]).contains("追加済み"));
    lab.success(&["plugin", "block", "configure", "~/bundle", "--set", "mode=drop"]);
    lab.success(&["plugin", "block", "validate", lab.package.to_str().unwrap()]);
    lab.success(&["plugin", "block", "update", "block-fixture"]);
    assert!(lab.success(&["plugin", "block", "list"]).contains("block-fixture\tblock"));
    assert!(!lab.success(&["plugin", "relay", "list"]).contains("block-fixture"));
    lab.success(&["plugin", "block", "del", "./bundle"]);
    assert!(lab.package.join("plugin.json").exists());
    assert!(lab.package.join("amitoki-plugin-block-fixture").exists());
    assert!(!lab.store.join("block-fixture").exists());
    assert!(lab.store.join(".config/block-fixture.json").exists());
}

#[test]
fn a_directory_removed_after_install_can_still_identify_the_registered_copy() {
    let lab = Lab::new();
    lab.success(&["plugin", "block", "add", "./bundle"]);
    fs::remove_dir_all(&lab.package).unwrap();
    lab.success(&["plugin", "block", "describe", "./bundle"]);
    lab.success(&["plugin", "block", "del", "./bundle"]);
}

#[test]
fn wrong_kinds_unknown_names_and_corrupted_updates_leave_the_installed_plugin_unchanged() {
    let lab = Lab::new();
    lab.failure(&["plugin", "relay", "add", "./bundle"]);
    lab.failure(&["plugin", "block", "add", "bundle"]);
    lab.success(&["plugin", "block", "add", "./bundle"]);
    for operation in ["describe", "configure", "validate", "del", "update"] {
        lab.failure(&["plugin", "relay", operation, "block-fixture"]);
    }
    let before = lab.installed();
    fs::write(lab.package.join(&before.binary), "broken").unwrap();
    lab.failure(&["plugin", "block", "update", "./bundle"]);
    before.verify(&lab.store.join("block-fixture")).unwrap();
}

#[test]
fn github_url_management_resolves_offline_and_never_installs_an_unknown_source() {
    let lab = Lab::new();
    lab.success(&["plugin", "block", "add", "./bundle"]);
    let mut package = lab.installed();
    package.source = Some("example/my-block@v0.1.0".into());
    lab.save_package(&lab.store.join("block-fixture"), &package);
    lab.success(&["plugin", "block", "describe", "https://github.com/example/my-block.git"]);
    lab.success(&[
        "plugin",
        "block",
        "validate",
        "https://github.com/EXAMPLE/my-block/releases/tag/v0.1.0",
    ]);
    lab.failure(&["plugin", "block", "del", "https://github.com/example/not-installed"]);
    lab.success(&["plugin", "block", "del", "https://github.com/example/my-block"]);
}

#[test]
fn legacy_install_update_and_remove_commands_remain_usable() {
    let lab = Lab::new();
    lab.success(&["plugin", "add", "--path", "bundle"]);
    lab.success(&["plugin", "update", "block-fixture", "--path", "bundle"]);
    lab.success(&["plugin", "configure", "block-fixture", "--set", "mode=pass"]);
    lab.success(&["plugin", "remove", "block-fixture"]);
}

#[test]
fn local_block_tests_report_outputs_and_rejections_without_installing_or_persisting_options() {
    let lab = Lab::new();
    write_capture(&lab.root.path().join("input.pcap"), &[vec![1; 14], vec![1; 13]]);
    let pass = reports(&lab.success(&["plugin", "block", "test", "./bundle", "--pcap", "input.pcap", "--json"]));
    assert_eq!(pass[0]["terminals"], json!(["output:pass"]));
    assert!(pass[1]["rejection"].is_string());
    let drop = reports(&lab.success(&[
        "plugin",
        "block",
        "test",
        "./bundle",
        "--pcap",
        "input.pcap",
        "--set",
        "mode=drop",
        "--json",
    ]));
    assert_eq!(drop[0]["terminals"], json!([]));
    assert!(!lab.store.exists());
    lab.success(&["plugin", "block", "add", "./bundle"]);
    lab.success(&["plugin", "block", "configure", "block-fixture", "--set", "mode=drop"]);
    let pass = reports(&lab.success(&[
        "plugin",
        "block",
        "test",
        "block-fixture",
        "--pcap",
        "input.pcap",
        "--set",
        "mode=pass",
        "--json",
    ]));
    assert_eq!(pass[0]["terminals"], json!(["output:pass"]));
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(lab.store.join(".config/block-fixture.json")).unwrap()).unwrap();
    assert_eq!(saved["mode"], "drop");
}

#[test]
fn block_failure_is_reported_and_causes_a_failed_test() {
    let lab = Lab::new();
    write_capture(&lab.root.path().join("input.pcap"), &[vec![1; 14]]);
    let output = lab.command(&[
        "plugin",
        "block",
        "test",
        "./bundle",
        "--pcap",
        "input.pcap",
        "--set",
        "mode=crash",
        "--json",
    ]);
    assert!(!output.status.success());
    let trace = reports(&String::from_utf8(output.stdout).unwrap());
    assert!(trace[0]["error"].is_string());
    assert!(trace[0]["steps"][0]["error"].is_string());
}

fn pipeline(lab: &Lab, mode: &str, policy: &str) {
    use sha2::{Digest, Sha256};
    lab.success(&["plugin", "block", "add", "./bundle"]);
    let relay_path = lab.root.path().join("relay");
    fs::create_dir(&relay_path).unwrap();
    let mut relay = lab.installed();
    relay.manifest.name = "offline-relay".into();
    relay.manifest.block = None;
    relay.binary = "amitoki-plugin-offline-relay".into();
    let executable = b"#!/bin/sh\ntouch relay-was-started\nexit 99\n";
    relay.sha256 = format!("{:x}", Sha256::digest(executable));
    fs::write(relay_path.join(&relay.binary), executable).unwrap();
    lab.save_package(&relay_path, &relay);
    lab.success(&["plugin", "relay", "add", "./relay"]);
    let timeout = if mode == "delay" { 50 } else { 10_000 };
    fs::write(
        lab.root.path().join("pipeline.toml"),
        format!(
            r#"
node_id="test"
channel="test"
interface="this-nic-does-not-exist"
[engine]
operation_timeout_ms={timeout}
[firewall]
policy="whitelist"
rules=[{{ type="EtherType", value=257 }}]
[[pipeline.relays]]
id="wire"
plugin="offline-relay"
[[pipeline.blocks]]
id="first"
plugin="block-fixture"
on_error="{policy}"
[pipeline.blocks.options]
mode="{mode}"
[[pipeline.blocks]]
id="second"
plugin="block-fixture"
[[pipeline.routes]]
from="capture"
to=["first", "wire"]
[[pipeline.routes]]
from="first.pass"
to=["second"]
[[pipeline.routes]]
from="second.pass"
to=["wire"]
[[pipeline.routes]]
from="wire.received"
to=["inject"]
"#
        ),
    )
    .unwrap();
    write_capture(&lab.root.path().join("input.pcap"), &[vec![1; 14], vec![2; 14]]);
}

#[test]
fn replay_traces_analysis_and_core_filtering_without_opening_a_nic_or_starting_relays() {
    let lab = Lab::new();
    pipeline(&lab, "pass", "stop");
    let output = lab.success(&["debug", "replay", "--config", "pipeline.toml", "--pcap", "input.pcap", "--json"]);
    let trace = reports(&output);
    assert_eq!(trace[0]["steps"][1]["annotations"]["input"]["instance"], "first");
    assert_eq!(trace[0]["steps"][0]["ports"][0]["to"], json!(["second"]));
    assert_eq!(trace[0]["terminals"], json!(["wire"]));
    assert!(trace[1]["rejection"].as_str().unwrap().contains("firewall"));
    assert!(!lab.root.path().join("relay-was-started").exists());
    let received = reports(&lab.success(&[
        "debug",
        "replay",
        "--config",
        "pipeline.toml",
        "--pcap",
        "input.pcap",
        "--source",
        "wire.received",
        "--json",
    ]));
    assert_eq!(received[0]["terminals"], json!(["inject"]));
    assert!(!lab.root.path().join("relay-was-started").exists());
}

#[test]
fn replay_respects_stop_and_drop_branch_when_blocks_fail_or_time_out() {
    for (mode, policy) in [("crash", "stop"), ("invalid", "stop"), ("delay", "drop_branch")] {
        let lab = Lab::new();
        pipeline(&lab, mode, policy);
        let output = lab.command(&["debug", "replay", "--config", "pipeline.toml", "--pcap", "input.pcap", "--json"]);
        let trace = reports(&String::from_utf8(output.stdout).unwrap());
        assert!(trace[0]["steps"][0]["error"].is_string());
        if policy == "stop" {
            assert!(!output.status.success());
            assert!(trace[0]["error"].is_string());
        } else {
            assert!(output.status.success());
            assert_eq!(trace[0]["terminals"], json!(["wire"]));
            assert!(trace[0]["error"].is_null());
        }
    }
}

#[test]
fn compare_ignores_processing_time_but_detects_changed_outputs_and_missing_packets() {
    let lab = Lab::new();
    write_capture(&lab.root.path().join("input.pcap"), &[vec![1; 14]]);
    let output = lab.success(&["plugin", "block", "test", "./bundle", "--pcap", "input.pcap", "--json"]);
    fs::write(lab.root.path().join("before.jsonl"), &output).unwrap();
    let mut report = reports(&output).remove(0);
    report["steps"][0]["elapsed_us"] = json!(999999);
    fs::write(lab.root.path().join("after.jsonl"), report.to_string()).unwrap();
    lab.success(&["debug", "compare", "before.jsonl", "after.jsonl"]);
    report["steps"][0]["annotations"]["changed"] = json!(true);
    fs::write(lab.root.path().join("after.jsonl"), report.to_string()).unwrap();
    lab.failure(&["debug", "compare", "before.jsonl", "after.jsonl"]);
    fs::write(lab.root.path().join("after.jsonl"), "").unwrap();
    lab.failure(&["debug", "compare", "before.jsonl", "after.jsonl"]);
}
