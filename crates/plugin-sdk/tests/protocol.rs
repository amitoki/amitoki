use amitoki_plugin_sdk::{
    wire::{read_message, write_message, Request, MAX_MESSAGE_BYTES},
    PluginManifest, PROTOCOL_VERSION,
};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn oversized_length_is_rejected_before_reading_the_payload() {
    let (mut writer, mut reader) = tokio::io::duplex(64);
    writer.write_u32(MAX_MESSAGE_BYTES as u32 + 1).await.unwrap();
    assert_eq!(read_message::<Request>(&mut reader).await.err().unwrap().kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn adjacent_messages_keep_their_boundaries() {
    let (mut writer, mut reader) = tokio::io::duplex(1024);
    write_message(&mut writer, &Request::Describe).await.unwrap();
    write_message(&mut writer, &Request::Receive { limit: 3 }).await.unwrap();
    assert!(matches!(read_message::<Request>(&mut reader).await.unwrap(), Request::Describe));
    assert!(matches!(read_message::<Request>(&mut reader).await.unwrap(), Request::Receive { limit: 3 }));
}

#[test]
fn invalid_options_are_rejected_without_echoing_secret_values() {
    let manifest = PluginManifest {
        packets: vec![],
        name: "example".into(),
        version: "1.0.0".into(),
        protocol_version: PROTOCOL_VERSION,
        description: String::new(),
        block: None,
        config_schema: serde_json::json!({"type":"object", "additionalProperties":false, "properties":{"count":{"type":"integer","minimum":1}}}),
    };
    assert!(manifest.validate_options(&serde_json::json!({"count":2})).is_ok());
    let error = manifest.validate_options(&serde_json::json!({"count":"secret-value"})).unwrap_err();
    assert!(!error.to_string().contains("secret-value"));
    assert!(manifest.validate_options(&serde_json::json!({"typo":2})).is_err());
}

#[test]
fn legacy_stage_output_remains_compatible_and_rewrites_require_declaration() {
    use amitoki_plugin_sdk::block::{BlockDefinition, BlockOutput};
    let definition: BlockDefinition = serde_json::from_value(serde_json::json!({"outputs":["pass"]})).unwrap();
    let mut output: BlockOutput = serde_json::from_value(serde_json::json!({"ports":["pass"],"annotations":{}})).unwrap();
    assert!(!definition.rewrite);
    assert!(output.bytes.is_none());
    assert!(serde_json::to_value(&definition).unwrap().get("rewrite").is_none());
    assert!(serde_json::to_value(&output).unwrap().get("bytes").is_none());
    output.bytes = Some(vec![0; 14].into());
    assert!(output.validate(&definition).is_err());
    let definition = BlockDefinition { rewrite: true, ..definition };
    assert!(output.validate(&definition).is_ok());
    output.bytes = Some(vec![0; 13].into());
    assert!(output.validate(&definition).is_err());
    output.bytes = Some(vec![0; 65536].into());
    assert!(output.validate(&definition).is_err());
    assert!(serde_json::from_value::<BlockOutput>(serde_json::json!({"ports":["pass"],"annotations":{},"id":"untrusted"})).is_err());
}
