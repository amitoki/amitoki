use amitoki::{config::AppConfig, engine::EngineConfig};

#[test]
fn example_configuration_loads_without_database_credentials() {
    let config = AppConfig::load(std::path::Path::new("amitoki.example.toml")).unwrap();
    assert_eq!(config.relay.plugin, "postgres");
    assert_eq!(config.engine.batch_size, 128);
}

#[test]
fn a_non_database_plugin_needs_no_postgres_configuration() {
    let config: AppConfig = toml::from_str("node_id='a'\nchannel='lan'\ninterface='eth0'\n[relay]\nplugin='memory'\n").unwrap();
    assert_eq!(config.relay.options, serde_json::json!({}));
}

#[test]
fn misspelled_common_configuration_is_rejected() {
    let text = "node_id='a'\nchannel='lan'\ninterface='eth0'\n[relay]\nplugin='memory'\n[engine]\nbatch_szie=10\n";
    assert!(toml::from_str::<AppConfig>(text).is_err());
}

#[test]
fn zero_queue_capacity_or_busy_polling_is_rejected() {
    assert!(EngineConfig {
        queue_capacity: 0,
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(EngineConfig {
        poll_interval_ms: 0,
        ..Default::default()
    }
    .validate()
    .is_err());
}
