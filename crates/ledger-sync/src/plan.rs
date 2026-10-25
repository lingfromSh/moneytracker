//! Three-way diff: what changed here, what changed there, what to do.
//!
//! `base` is the hash each file had when this device last finished a sync, so
//! "changed" is decided against that shared ancestor rather than against each
//! other. Anything both sides changed differently is a conflict and blocks the
//! whole sync — the user picks a side, then the sync is recomputed.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Conflict, Entry, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Upload,
    Download,
    DeleteRemote,
    DeleteLocal,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Upload => "上传",
            Action::Download => "下载",
            Action::DeleteRemote => "云端删除",
            Action::DeleteLocal => "本地删除",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub uploads: Vec<String>,
    pub downloads: Vec<String>,
    pub delete_remote: Vec<String>,
    pub delete_local: Vec<String>,
    pub conflicts: Vec<Conflict>,
}

impl Plan {
    pub fn is_noop(&self) -> bool {
        self.uploads.is_empty()
            && self.downloads.is_empty()
            && self.delete_remote.is_empty()
            && self.delete_local.is_empty()
            && self.conflicts.is_empty()
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    pub fn conflict(&self, rel: &str) -> Option<&Conflict> {
        self.conflicts.iter().find(|conflict| conflict.rel == rel)
    }

    /// Moves every resolved conflict into the upload/download lists.
    pub fn resolve(
        &mut self,
        choices: &BTreeMap<String, Side>,
        local: &BTreeMap<String, Entry>,
        remote: &BTreeMap<String, Entry>,
    ) {
        let mut kept: Vec<Conflict> = Vec::new();
        for conflict in self.conflicts.drain(..) {
            match choices.get(&conflict.rel) {
                Some(Side::Local) => {
                    if local.contains_key(&conflict.rel) {
                        self.uploads.push(conflict.rel);
                    } else {
                        self.delete_remote.push(conflict.rel);
                    }
                }
                Some(Side::Remote) => {
                    if remote.contains_key(&conflict.rel) {
                        self.downloads.push(conflict.rel);
                    } else {
                        self.delete_local.push(conflict.rel);
                    }
                }
                None => kept.push(conflict),
            }
        }
        self.conflicts = kept;
        self.uploads.sort();
        self.uploads.dedup();
        self.downloads.sort();
        self.downloads.dedup();
        self.delete_remote.sort();
        self.delete_remote.dedup();
        self.delete_local.sort();
        self.delete_local.dedup();
    }

    pub fn changes(&self) -> usize {
        self.uploads.len()
            + self.downloads.len()
            + self.delete_remote.len()
            + self.delete_local.len()
    }
}

fn stamp(entry: &Entry) -> String {
    chrono::DateTime::from_timestamp_millis(entry.mtime_ms)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn diff(
    local: &BTreeMap<String, Entry>,
    remote: &BTreeMap<String, Entry>,
    base: &BTreeMap<String, String>,
) -> Plan {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    keys.extend(local.keys());
    keys.extend(remote.keys());
    keys.extend(base.keys());

    let mut plan = Plan::default();
    for rel in keys {
        let local_hash = local.get(rel).map(|entry| entry.hash.clone());
        let remote_hash = remote.get(rel).map(|entry| entry.hash.clone());
        let base_hash = base.get(rel).cloned();

        if local_hash == remote_hash {
            continue;
        }
        if local_hash.is_some() && remote_hash.is_none() && base_hash.is_none() {
            plan.uploads.push(rel.clone());
            continue;
        }
        if local_hash.is_none() && remote_hash.is_some() && base_hash.is_none() {
            plan.downloads.push(rel.clone());
            continue;
        }
        if base_hash == local_hash {
            // Only the remote moved.
            if remote_hash.is_some() {
                plan.downloads.push(rel.clone());
            } else {
                plan.delete_local.push(rel.clone());
            }
            continue;
        }
        if base_hash == remote_hash {
            // Only this device moved.
            if local_hash.is_some() {
                plan.uploads.push(rel.clone());
            } else {
                plan.delete_remote.push(rel.clone());
            }
            continue;
        }
        plan.conflicts.push(Conflict {
            rel: rel.clone(),
            local_hash,
            remote_hash,
            base_hash,
            local_mtime: local.get(rel).map(stamp),
            remote_mtime: remote.get(rel).map(stamp),
            local_bytes: local.get(rel).map(|entry| entry.size as usize),
            remote_bytes: remote.get(rel).map(|entry| entry.size as usize),
        });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str) -> Entry {
        Entry {
            hash: hash.to_string(),
            size: hash.len() as u64,
            mtime_ms: 1_700_000_000_000,
        }
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, Entry> {
        pairs
            .iter()
            .map(|(rel, hash)| ((*rel).to_string(), entry(hash)))
            .collect()
    }

    fn base(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(rel, hash)| ((*rel).to_string(), (*hash).to_string()))
            .collect()
    }

    #[test]
    fn identical_sides_do_nothing() {
        let local = map(&[("a.ledger", "h1")]);
        let plan = diff(&local, &local, &base(&[("a.ledger", "h1")]));
        assert!(plan.is_noop());
    }

    #[test]
    fn a_fresh_file_is_uploaded() {
        let plan = diff(
            &map(&[("a.ledger", "h1")]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(plan.uploads, vec!["a.ledger"]);
        assert!(!plan.has_conflicts());
    }

    #[test]
    fn a_remote_only_file_is_downloaded() {
        let plan = diff(
            &BTreeMap::new(),
            &map(&[("a.ledger", "h1")]),
            &BTreeMap::new(),
        );
        assert_eq!(plan.downloads, vec!["a.ledger"]);
    }

    #[test]
    fn only_the_remote_moved() {
        let plan = diff(
            &map(&[("a.ledger", "h1")]),
            &map(&[("a.ledger", "h2")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(plan.downloads, vec!["a.ledger"]);
    }

    #[test]
    fn only_this_device_moved() {
        let plan = diff(
            &map(&[("a.ledger", "h2")]),
            &map(&[("a.ledger", "h1")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(plan.uploads, vec!["a.ledger"]);
    }

    #[test]
    fn both_changed_the_same_way() {
        let plan = diff(
            &map(&[("a.ledger", "h2")]),
            &map(&[("a.ledger", "h2")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert!(plan.is_noop());
    }

    #[test]
    fn both_changed_differently_conflicts() {
        let plan = diff(
            &map(&[("a.ledger", "h2")]),
            &map(&[("a.ledger", "h3")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(plan.conflicts.len(), 1);
        let conflict = &plan.conflicts[0];
        assert_eq!(conflict.rel, "a.ledger");
        assert_eq!(conflict.local_hash.as_deref(), Some("h2"));
        assert_eq!(conflict.remote_hash.as_deref(), Some("h3"));
        assert_eq!(conflict.base_hash.as_deref(), Some("h1"));
        assert!(plan.uploads.is_empty() && plan.downloads.is_empty());
    }

    #[test]
    fn a_new_file_on_both_sides_conflicts() {
        let plan = diff(
            &map(&[("a.ledger", "h1")]),
            &map(&[("a.ledger", "h2")]),
            &BTreeMap::new(),
        );
        assert_eq!(plan.conflicts.len(), 1);
    }

    #[test]
    fn deletions_flow_from_the_side_that_deleted() {
        let gone_here = diff(
            &BTreeMap::new(),
            &map(&[("a.ledger", "h1")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(gone_here.delete_remote, vec!["a.ledger"]);
        let gone_there = diff(
            &map(&[("a.ledger", "h1")]),
            &BTreeMap::new(),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(gone_there.delete_local, vec!["a.ledger"]);
    }

    #[test]
    fn delete_versus_edit_conflicts() {
        let plan = diff(
            &BTreeMap::new(),
            &map(&[("a.ledger", "h2")]),
            &base(&[("a.ledger", "h1")]),
        );
        assert_eq!(plan.conflicts.len(), 1);
        assert!(plan.conflicts[0].local_hash.is_none());
    }

    #[test]
    fn resolving_keeps_the_chosen_side() {
        let local = map(&[("a.ledger", "h2")]);
        let remote = map(&[("a.ledger", "h3")]);
        let mut plan = diff(&local, &remote, &base(&[("a.ledger", "h1")]));
        let mut choices = BTreeMap::new();
        choices.insert("a.ledger".to_string(), Side::Local);
        plan.resolve(&choices, &local, &remote);
        assert!(plan.conflicts.is_empty());
        assert_eq!(plan.uploads, vec!["a.ledger"]);

        let mut plan = diff(&local, &remote, &base(&[("a.ledger", "h1")]));
        let mut choices = BTreeMap::new();
        choices.insert("a.ledger".to_string(), Side::Remote);
        plan.resolve(&choices, &local, &remote);
        assert_eq!(plan.downloads, vec!["a.ledger"]);
    }

    #[test]
    fn resolving_a_delete_against_an_edit() {
        let local: BTreeMap<String, Entry> = BTreeMap::new();
        let remote = map(&[("a.ledger", "h2")]);
        let mut plan = diff(&local, &remote, &base(&[("a.ledger", "h1")]));
        let mut choices = BTreeMap::new();
        choices.insert("a.ledger".to_string(), Side::Remote);
        plan.resolve(&choices, &local, &remote);
        assert_eq!(plan.downloads, vec!["a.ledger"]);

        let mut plan = diff(&local, &remote, &base(&[("a.ledger", "h1")]));
        let mut choices = BTreeMap::new();
        choices.insert("a.ledger".to_string(), Side::Local);
        plan.resolve(&choices, &local, &remote);
        assert_eq!(plan.delete_remote, vec!["a.ledger"]);
    }

    #[test]
    fn unresolved_conflicts_stay() {
        let local = map(&[("a.ledger", "h2")]);
        let remote = map(&[("a.ledger", "h3")]);
        let mut plan = diff(&local, &remote, &base(&[("a.ledger", "h1")]));
        plan.resolve(&BTreeMap::new(), &local, &remote);
        assert_eq!(plan.conflicts.len(), 1);
    }
}
