//! A run-wide, thread-shared memo.
//!
//! `portage_repo::is_visible` and `effective_use_flags` keep their memos in
//! `thread_local!`s, so with N workers the same candidate is judged up to N
//! times. Nothing in the config changes during a run, so
//! `cat/pkg-ver:slot/sub::repo` identifies a candidate's verdict.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::RwLock;

const SHARDS: usize = 64;

pub struct Memo<V> {
    hasher: RandomState,
    shards: Vec<RwLock<HashMap<String, V>>>,
}

impl<V: Clone> Memo<V> {
    pub fn new() -> Self {
        Self {
            hasher: RandomState::new(),
            shards: (0..SHARDS).map(|_| RwLock::default()).collect(),
        }
    }

    /// The value for `key`, computed by `f` once for all threads (two threads
    /// racing on a new key may both compute it; the results are equal).
    pub fn get_or_compute(&self, key: String, f: impl FnOnce() -> V) -> V {
        let shard = &self.shards[self.hasher.hash_one(&key) as usize % SHARDS];
        if let Some(v) = shard.read().expect("memo lock").get(&key) {
            return v.clone();
        }
        let v = f();
        shard.write().expect("memo lock").insert(key, v.clone());
        v
    }
}
