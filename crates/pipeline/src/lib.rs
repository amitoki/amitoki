//! Rustで接続図を作り、実装をリンクせずにプラグイン名で参照する。
mod definition;
pub use definition::*;
use std::{io::Write, path::Path};

pub struct Pipeline {
    definition: PipelineConfig,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            definition: PipelineConfig {
                relays: vec![],
                blocks: vec![],
                routes: vec![],
            },
        }
    }

    pub fn add_stage(mut self, stage: Stage) -> Self {
        self.definition.blocks.push(stage);
        self
    }

    pub fn add_relay(mut self, relay: RelayInstance) -> Self {
        self.definition.relays.push(relay);
        self
    }

    /// 順序と分岐は接続で指定する。空の接続先は明示的な破棄。
    pub fn connect(mut self, from: impl Into<String>, to: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.definition.routes.push(Route {
            from: from.into(),
            to: to.into_iter().map(Into::into).collect(),
        });
        self
    }

    pub fn discard(self, from: impl Into<String>) -> Self {
        self.connect(from, std::iter::empty::<String>())
    }

    pub fn build(self) -> Result<PipelineConfig, String> {
        self.definition.validate()?;
        Ok(self.definition)
    }

    /// reloadが途中のJSONを読まないよう、同じディレクトリ内で置き換える。
    /// ポート・循環・ループ抑制の検証はプラグイン定義を持つ本体が行う。
    pub fn write(self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        let definition = self.build()?;
        let path = path.as_ref();
        let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut output, &definition)?;
        writeln!(output)?;
        output.as_file().sync_all()?;
        output.persist(path)?;
        Ok(())
    }
}

impl Stage {
    pub fn plugin(id: impl Into<String>, plugin: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            plugin: plugin.into(),
            options: serde_json::json!({}),
            on_error: ErrorPolicy::Stop,
        }
    }

    pub fn options(mut self, options: serde_json::Value) -> Self {
        self.options = options;
        self
    }

    pub fn on_error(mut self, policy: ErrorPolicy) -> Self {
        self.on_error = policy;
        self
    }
}

impl RelayInstance {
    pub fn plugin(id: impl Into<String>, plugin: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            plugin: plugin.into(),
            channel: None,
            options: serde_json::json!({}),
        }
    }

    pub fn options(mut self, options: serde_json::Value) -> Self {
        self.options = options;
        self
    }

    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }
}
