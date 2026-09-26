use amitoki_relay::{PluginRegistry, RelayError};

/// 組み込みプラグインを登録する唯一の場所。エンジンは具体的な実装を参照しない。
pub fn builtin_plugins() -> Result<PluginRegistry, RelayError> {
    let mut registry = PluginRegistry::default();
    registry.register(amitoki_relay_memory::MemoryPlugin::default())?;
    Ok(registry)
}
