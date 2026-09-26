//! 解析の完了は取得元ごと、NICへの注入抑制は全経路共通で記録する。
use std::{
    collections::{HashSet, VecDeque},
    hash::Hash,
};
use uuid::Uuid;

// P2P側の重複除去と同じ件数を保持し、長時間稼働でも履歴を増やし続けない。
const RECENT_FRAME_CAPACITY: usize = 65_536;
#[derive(Default)]
pub(super) struct DeliveryHistory {
    pub completed: RecentSet<(usize, Uuid)>,
    pub nic_seen: RecentSet<Uuid>,
}
#[derive(Default)]
pub(super) struct RecentSet<Key> {
    keys: HashSet<Key>,
    order: VecDeque<Key>,
}
impl<Key: Eq + Hash + Copy> RecentSet<Key> {
    pub fn contains(&self, key: &Key) -> bool {
        self.keys.contains(key)
    }
    pub fn insert(&mut self, key: Key) {
        if !self.keys.insert(key) {
            return;
        }
        self.order.push_back(key);
        if self.order.len() > RECENT_FRAME_CAPACITY {
            self.keys.remove(&self.order.pop_front().expect("nonempty history"));
        }
    }
}
