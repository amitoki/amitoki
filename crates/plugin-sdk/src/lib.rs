//! 外部プロセス型プラグインの設定定義と、長さ付きMessagePack通信。
mod client;
mod manifest;
mod server;
pub mod wire;

pub use amitoki_relay as relay;
pub use client::ProcessRelay;
pub use manifest::{PluginManifest, PROTOCOL_VERSION};
pub use server::serve;
