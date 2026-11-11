//! Changes between states and merging them, as NipaPlay's incremental
//! sync does: a state is categories of keyed values; a change sets or
//! removes one key. Two devices that changed different keys both keep
//! their changes; where both changed one key, a record that says when it
//! happened (an `at` field) keeps the later one, and anything else keeps
//! the change of the device syncing now, which every device then takes.

use crate::format::{Op, State};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The changes that turn `before` into `after`, in category and key order.
pub fn diff(before: &State, after: &State) -> Vec<Op> {
    let empty = BTreeMap::new();
    let categories: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let mut ops = Vec::new();
    for category in categories {
        let old = before.get(category).unwrap_or(&empty);
        let new = after.get(category).unwrap_or(&empty);
        let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
        for key in keys {
            match (old.get(key), new.get(key)) {
                (Some(_), None) => ops.push(Op {
                    category: category.clone(),
                    key: key.clone(),
                    value: None,
                }),
                (was, Some(value)) if was != Some(value) => ops.push(Op {
                    category: category.clone(),
                    key: key.clone(),
                    value: Some(value.clone()),
                }),
                _ => {}
            }
        }
    }
    ops
}

pub fn apply(state: &mut State, ops: impl IntoIterator<Item = Op>) {
    for op in ops {
        let entries = state.entry(op.category).or_default();
        match op.value {
            Some(value) => {
                entries.insert(op.key, value);
            }
            None => {
                entries.remove(&op.key);
            }
        }
    }
    state.retain(|_, entries| !entries.is_empty());
}

/// When a record happened, if it says.
fn at(value: Option<&Value>) -> Option<i64> {
    value?.int_of("at")
}

/// Whether the local side of a key both sides changed wins: the later of
/// two records that say when; otherwise `local_wins`.
fn local_wins(category: &str, local: Option<&Value>, remote: Option<&Value>, local_wins: bool) -> bool {
    if crate::catalog::CATEGORIES.contains(&category) {
        let ours = local.is_some_and(crate::catalog::is_manual);
        let theirs = remote.is_some_and(crate::catalog::is_manual);
        if ours != theirs { return ours; }
    }
    match (at(local), at(remote)) {
        (Some(local), Some(remote)) if local != remote => local > remote,
        // A record removed on one side and changed on the other stays.
        _ if local.is_none() && at(remote).is_some() => false,
        _ if remote.is_none() && at(local).is_some() => true,
        _ => local_wins,
    }
}

/// The first sync of two sides with nothing in common yet: every key of
/// either, and where both have a key, the remote one unless the local
/// record is later. Nothing is removed.
pub fn merge_first(remote: &State, local: &State) -> State {
    let mut merged = remote.clone();
    for (category, entries) in local {
        let target = merged.entry(category.clone()).or_default();
        for (key, value) in entries {
            match target.get(key) {
                None => {
                    target.insert(key.clone(), value.clone());
                }
                Some(theirs) if theirs != value && local_wins(category, Some(value), Some(theirs), false) => {
                    target.insert(key.clone(), value.clone());
                }
                Some(_) => {}
            }
        }
    }
    merged
}

/// Both sides changed since `base`, which they last agreed on: the remote
/// state with the local changes on it, by the rules above.
pub fn merge_three(base: &State, remote: &State, local: &State) -> State {
    let remote_changed: BTreeSet<(String, String)> = diff(base, remote)
        .into_iter()
        .map(|op| (op.category, op.key))
        .collect();
    let accepted: Vec<Op> = diff(base, local)
        .into_iter()
        .filter(|op| {
            if !remote_changed.contains(&(op.category.clone(), op.key.clone())) {
                return true;
            }
            let theirs = remote
                .get(&op.category)
                .and_then(|entries| entries.get(&op.key));
            local_wins(&op.category, op.value.as_ref(), theirs, true)
        })
        .collect();
    let mut merged = remote.clone();
    apply(&mut merged, accepted);
    merged
}

/// Only `categories` of `state`.
pub fn select(state: &State, categories: &BTreeSet<String>) -> State {
    state
        .iter()
        .filter(|(category, _)| categories.contains(*category))
        .map(|(category, entries)| (category.clone(), entries.clone()))
        .collect()
}
