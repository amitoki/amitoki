use super::{ManagerResult, Package};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginKind {
    Relay,
    Block,
}

impl PluginKind {
    pub fn of(package: &Package) -> Self {
        if package.manifest.block.is_some() {
            Self::Block
        } else {
            Self::Relay
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Relay => "relay",
            Self::Block => "stage",
        }
    }

    pub fn check(self, package: &Package) -> ManagerResult<()> {
        if Self::of(package) != self {
            return Err(format!("{}は{}プラグインではありません", package.manifest.name, self.name()).into());
        }
        Ok(())
    }
}
