//! 経路を起動時に検証し、実行時には添字で参照する。
use super::config::{PipelineConfig, MAX_VISITS};
use amitoki_plugin_sdk::block::BlockDefinition;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    Block(usize),
    Relay(usize),
    Inject,
}

#[derive(Debug)]
pub struct Graph {
    pub capture: Vec<Destination>,
    pub received: Vec<Vec<Destination>>,
    pub outputs: Vec<HashMap<String, Vec<Destination>>>,
    pub order: Vec<usize>,
}
impl Graph {
    pub fn compile(config: &PipelineConfig, definitions: &[BlockDefinition]) -> Result<Self, String> {
        config.validate()?;
        if definitions.len() != config.blocks.len() {
            return Err("ブロックの定義数が一致しません".into());
        }
        let mut destinations = HashMap::from([("inject".to_owned(), Destination::Inject)]);
        for (index, block) in config.blocks.iter().enumerate() {
            destinations.insert(block.id.clone(), Destination::Block(index));
        }
        for (index, relay) in config.relays.iter().enumerate() {
            destinations.insert(relay.id.clone(), Destination::Relay(index));
        }
        let mut routes: HashMap<_, _> = config.routes.iter().map(|route| (route.from.clone(), route.to.iter().map(|name| destinations[name]).collect::<Vec<_>>())).collect();
        let capture = take_route(&mut routes, "capture")?;
        let received = config.relays.iter().map(|relay| take_route(&mut routes, &format!("{}.received", relay.id))).collect::<Result<_, _>>()?;
        let mut outputs = Vec::new();
        for (block, definition) in config.blocks.iter().zip(definitions) {
            definition.validate().map_err(|error| error.to_string())?;
            let mut ports = HashMap::new();
            for port in &definition.outputs {
                ports.insert(port.clone(), take_route(&mut routes, &format!("{}.{port}", block.id))?);
            }
            outputs.push(ports);
        }
        if !routes.is_empty() {
            return Err("未定義の出力ポートがあります".into());
        }
        let mut graph = Self {
            capture,
            received,
            outputs,
            order: Vec::new(),
        };
        graph.order = graph.topological_order()?;
        graph.validate_paths()?;
        Ok(graph)
    }

    fn topological_order(&self) -> Result<Vec<usize>, String> {
        let mut pending = vec![0; self.outputs.len()];
        for ports in &self.outputs {
            for destination in ports.values().flatten() {
                if let Destination::Block(index) = destination {
                    pending[*index] += 1;
                }
            }
        }
        let mut ready: BinaryHeap<_> = pending.iter().enumerate().filter_map(|(index, count)| (*count == 0).then_some(Reverse(index))).collect();
        let mut order = Vec::new();
        while let Some(Reverse(index)) = ready.pop() {
            order.push(index);
            for destination in self.outputs[index].values().flatten() {
                if let Destination::Block(next) = destination {
                    pending[*next] -= 1;
                    if pending[*next] == 0 {
                        ready.push(Reverse(*next));
                    }
                }
            }
        }
        if order.len() != self.outputs.len() {
            return Err("ブロックを循環して接続できません".into());
        }
        Ok(order)
    }

    fn validate_paths(&self) -> Result<(), String> {
        let mut reachable = HashSet::new();
        for (root, receiving) in std::iter::once((&self.capture, false)).chain(self.received.iter().map(|root| (root, true))) {
            let mut pending = root.clone();
            let mut visits = 0;
            while let Some(destination) = pending.pop() {
                visits += 1;
                if visits > MAX_VISITS {
                    return Err("1パケットのグラフ展開が128回を超えます".into());
                }
                match destination {
                    Destination::Block(index) => {
                        reachable.insert(index);
                        pending.extend(self.outputs[index].values().flatten().copied());
                    },
                    Destination::Relay(_) if receiving => return Err("受信したパケットを別の中継へ再転送できません".into()),
                    Destination::Inject if !receiving => return Err("captureをinjectへ接続できません".into()),
                    _ => {},
                }
            }
        }
        if reachable.len() != self.outputs.len() {
            return Err("入力から到達できないブロックがあります".into());
        }
        Ok(())
    }
}
fn take_route(routes: &mut HashMap<String, Vec<Destination>>, source: &str) -> Result<Vec<Destination>, String> {
    routes.remove(source).ok_or_else(|| format!("経路を明示してください（破棄はto=[]）: {source}"))
}
