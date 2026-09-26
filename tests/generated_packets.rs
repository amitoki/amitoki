// 共通のCLI試験支援のうち、生成試験で使わない操作も含まれる。
#[allow(dead_code)]
#[path = "support/developer.rs"]
mod support;
use support::{reports, Lab};

#[test]
fn rust_packet_generation_is_repeatable_and_does_not_install_the_test_package() {
    let lab = Lab::new();
    let arguments = [
        "plugin",
        "stage",
        "test",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--count",
        "1000",
        "--seed",
        "42",
        "--json",
    ];
    let first = reports(&lab.success(&arguments));
    let second = reports(&lab.success(&arguments));
    assert_eq!(first.len(), 1000);
    for (left, right) in first.iter().zip(second.iter()) {
        assert_eq!(left["sha256"], right["sha256"]);
        assert!(left["rejection"].is_null());
        assert_eq!(left["terminals"], serde_json::json!(["output:pass"]));
    }
    assert!(!lab.store.exists());
    let other = reports(&lab.success(&[
        "plugin",
        "stage",
        "test",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--count",
        "1",
        "--seed",
        "43",
        "--json",
    ]));
    assert_ne!(first[0]["sha256"], other[0]["sha256"]);
}

#[test]
fn generation_rejects_unknown_definitions_and_invalid_field_ranges() {
    let lab = Lab::new();
    lab.failure(&["plugin", "stage", "test", "./bundle", "--packet", "missing"]);
    lab.failure(&[
        "plugin",
        "stage",
        "test",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--packet-set",
        "payload_bytes=999999",
    ]);
    lab.failure(&[
        "plugin",
        "stage",
        "test",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--packet-set",
        "min_temperature=90",
        "--packet-set",
        "max_temperature=10",
    ]);
}

#[test]
fn the_benchmark_counts_only_measured_packets_and_reports_generation_separately() {
    let lab = Lab::new();
    let report: serde_json::Value = serde_json::from_str(&lab.success(&[
        "plugin",
        "stage",
        "bench",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--count",
        "1000",
        "--warmup",
        "16",
        "--batch-size",
        "64",
        "--json",
    ]))
    .unwrap();
    assert_eq!(report["packets"], 1000);
    assert_eq!(report["batches"], 16);
    assert_eq!(report["warmup_packets"], 16);
    assert_eq!(report["output_packets_by_port"]["pass"], 1000);
    assert_eq!(report["rejected_packets"], 0);
    assert!(report["generation_seconds"].as_f64().unwrap() > 0.0);
    assert!(report["processing_seconds"].as_f64().unwrap() > 0.0);
    assert_eq!(report["errors"], 0);
}

#[test]
fn duration_and_rate_are_accepted_without_an_explicit_packet_count() {
    let lab = Lab::new();
    let report: serde_json::Value = serde_json::from_str(&lab.success(&[
        "plugin",
        "stage",
        "bench",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--duration",
        "1s",
        "--rate",
        "20",
        "--batch-size",
        "1",
        "--warmup",
        "0",
        "--json",
    ]))
    .unwrap();
    assert!(report["packets"].as_u64().unwrap() <= 20);
    assert!(report["packets"].as_u64().unwrap() > 0);
    assert!(report["elapsed_seconds"].as_f64().unwrap() >= 1.0);
}

#[test]
fn watch_uses_the_selected_store_for_a_separate_generator_and_forwards_packet_options() {
    let lab = Lab::new();
    lab.success(&["plugin", "--directory", "./generators", "stage", "add", "./bundle"]);
    let output = lab.success(&[
        "plugin",
        "--directory",
        "./generators",
        "stage",
        "watch",
        "./bundle",
        "--generator",
        "block-fixture",
        "--packet",
        "telemetry-v1",
        "--count",
        "3",
        "--seed",
        "21",
        "--packet-set",
        "payload_bytes=1400",
        "--once",
        "--json",
    ]);
    let watched = reports(&output);
    assert_eq!(watched.len(), 3);
    let direct = reports(&lab.success(&[
        "plugin",
        "stage",
        "test",
        "./bundle",
        "--packet",
        "telemetry-v1",
        "--count",
        "3",
        "--seed",
        "21",
        "--packet-set",
        "payload_bytes=1400",
        "--json",
    ]));
    for (watched, direct) in watched.iter().zip(direct) {
        assert_eq!(watched["sha256"], direct["sha256"]);
    }
}
