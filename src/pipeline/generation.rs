//! 入口で世代を固定し、配送・ACKが終わるまで旧Stageを保持する。
use super::{graph::Graph, plan::RunningBlock};
use std::sync::{Arc, RwLock, Weak};

// 連続reloadで、遅い配送が保持する子プロセスを無制限に増やさない。
const MAX_RETIRED_GENERATIONS: usize = 3;

pub struct PreparedPipeline {
    pub(crate) graph: Graph,
    pub(crate) blocks: Vec<RunningBlock>,
}

impl PreparedPipeline {
    pub fn new(config: &super::PipelineConfig, blocks: Vec<RunningBlock>) -> Result<Self, String> {
        let definitions = blocks.iter().map(|block| block.definition.clone()).collect::<Vec<_>>();
        Ok(Self {
            graph: Graph::compile(config, &definitions)?,
            blocks,
        })
    }
}

pub(crate) struct Generation {
    pub number: u64,
    pub graph: Graph,
    pub blocks: Vec<RunningBlock>,
}

impl Drop for Generation {
    fn drop(&mut self) {
        log::info!("Pipeline世代を終了します: generation={}", self.number);
    }
}

struct Generations {
    active: Arc<Generation>,
    retired: Vec<Weak<Generation>>,
}

pub(crate) struct GenerationStore(RwLock<Generations>);

impl GenerationStore {
    pub fn new(prepared: PreparedPipeline) -> Self {
        Self(RwLock::new(Generations {
            active: Arc::new(Generation {
                number: 1,
                graph: prepared.graph,
                blocks: prepared.blocks,
            }),
            retired: vec![],
        }))
    }

    pub fn active(&self) -> Arc<Generation> {
        self.0.read().expect("generation lock poisoned").active.clone()
    }

    pub fn replace(&self, prepared: PreparedPipeline) -> Result<u64, String> {
        let mut generations = self.0.write().expect("generation lock poisoned");
        generations.retired.retain(|generation| generation.strong_count() > 0);
        if generations.retired.len() >= MAX_RETIRED_GENERATIONS {
            return Err("旧Pipelineの処理が残っています。完了してからreloadしてください".into());
        }
        if prepared.graph.received.len() != generations.active.graph.received.len() {
            return Err("reloadではRelayの構成を変更できません".into());
        }
        let number = generations.active.number.checked_add(1).ok_or("Pipelineの世代番号が上限に達しました")?;
        let next = Arc::new(Generation {
            number,
            graph: prepared.graph,
            blocks: prepared.blocks,
        });
        let previous = std::mem::replace(&mut generations.active, next);
        generations.retired.push(Arc::downgrade(&previous));
        log::info!("Pipelineを切り替えました: generation={number} previous={}", previous.number);
        Ok(number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_pipeline() -> PreparedPipeline {
        let config = amitoki_pipeline::Pipeline::new().discard("capture").build().unwrap();
        PreparedPipeline::new(&config, vec![]).unwrap()
    }

    #[test]
    fn reload_limits_retained_generations_and_resumes_after_old_work_finishes() {
        let store = GenerationStore::new(empty_pipeline());
        let mut held = vec![];
        for expected in 2..=4 {
            held.push(store.active());
            assert_eq!(store.replace(empty_pipeline()).unwrap(), expected);
        }
        assert!(store.replace(empty_pipeline()).is_err());
        assert_eq!(store.active().number, 4);
        assert_eq!(held.iter().map(|generation| generation.number).collect::<Vec<_>>(), vec![1, 2, 3]);
        held.clear();
        assert_eq!(store.replace(empty_pipeline()).unwrap(), 5);
    }
}
