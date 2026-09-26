mod cli;
pub(crate) mod configure;
mod download;
mod kind;
mod operations;
mod package;
pub(crate) mod source;
mod store;

pub use cli::run_cli;
pub use kind::PluginKind;
pub use package::Package;
pub use store::PluginStore;
pub type ManagerResult<T> = Result<T, Box<dyn std::error::Error>>;
