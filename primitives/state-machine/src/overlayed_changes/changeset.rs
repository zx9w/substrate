// This file is part of Substrate.

// Copyright (C) 2017-2020 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License

//! Houses the code that implements the transactional overlay storage.

use super::{StorageKey, StorageValue};

use itertools::Itertools;
use std::collections::{HashSet, BTreeMap, BTreeSet};

const PROOF_DIRTY_KEYS: &str = "\
	We assume transactions are balanced. Every start of a transaction created one dirty
	keys element. This function is only called on transaction close. Therefore an element
	created when starting the transaction must exist; qed";

const PROOF_DIRTY_OVERLAY_VALUE: &str = "\
	A write to an OverlayedValue is recorded in the dirty key set. Before an OverlayedValues
	is removed its containing dirty set is removed. This function is only called for keys that
	are in the dirty set. Therefore the entry must exist; qed";

const PROOF_OVERLAY_NON_EMPTY: &str = "\
	An OverlayValue is always created with at least one transaction and dropped as soon
	as the last transaction is removed; qed";

#[derive(Debug, Default, Clone)]
#[cfg_attr(test, derive(PartialEq))]
struct InnerValue {
	/// Current value. None if value has been deleted.
	value: Option<StorageValue>,
	/// The set of extrinsic indices where the values has been changed.
	/// Is filled only if runtime has announced changes trie support.
	extrinsics: BTreeSet<u32>,
}

/// An overlay that contains all versions of a value for a specific key.
#[derive(Debug, Default, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct OverlayedValue {
	// The individual versions of that value.
	// One entry per transactions during that the value was actually written.
	transactions: Vec<InnerValue>,
}

/// Holds a set of changes with the ability modify them using nested transactions.
#[derive(Debug, Default, Clone)]
pub struct OverlayedChangeSet {
	/// Stores the changes that this overlay constitutes.
	changes: BTreeMap<StorageKey, OverlayedValue>,
	/// Stores which keys are dirty per transaction. Needed in order to determine which
	/// values to merge into the parent transaction on commit. The length of this vector
	/// therefore determines how many nested transactions are currently open (depth).
	dirty_keys: Vec<HashSet<StorageKey>>,
}

#[cfg(test)]
impl std::iter::FromIterator<(StorageKey, OverlayedValue)> for OverlayedChangeSet {
	fn from_iter<T: IntoIterator<Item = (StorageKey, OverlayedValue)>>(iter: T) -> Self {
		Self {
			changes: iter.into_iter().collect(),
			.. Default::default()
		}
	}
}

#[cfg(test)]
impl From<Option<StorageValue>> for OverlayedValue {
	fn from(value: Option<StorageValue>) -> OverlayedValue {
		OverlayedValue {
			transactions: vec![InnerValue {
				value,
				.. Default::default()
			}]
		}
	}
}

impl OverlayedValue {
	/// The value as seen by the current transaction.
	pub fn value(&self) -> Option<&StorageValue> {
		self.transactions.last().expect(PROOF_OVERLAY_NON_EMPTY).value.as_ref()
	}

	/// Unique list of extrinsic indices which modified the value.
	pub fn extrinsics(&self) -> impl Iterator<Item=&u32> {
		self.transactions.iter().flat_map(|t| t.extrinsics.iter()).unique()
	}

	// Mutable reference to the most recent version.
	fn value_mut(&mut self) -> &mut Option<StorageValue> {
		&mut self.transactions.last_mut().expect(PROOF_OVERLAY_NON_EMPTY).value
	}

	// Remove the last version and return it.
	fn pop_transaction(&mut self) -> InnerValue {
		self.transactions.pop().expect(PROOF_OVERLAY_NON_EMPTY)
	}

	// Mutable reference to the set which holds the indices for the **current transaction only**.
	fn transaction_extrinsics_mut(&mut self) -> &mut BTreeSet<u32> {
		&mut self.transactions.last_mut().expect(PROOF_OVERLAY_NON_EMPTY).extrinsics
	}

	// Writes a new version of a value.
	//
	// This makes sure that the old version is not overwritten and can be properly
	// rolled back when required.
	fn set(
		&mut self,
		value: Option<StorageValue>,
		first_write_in_tx: bool,
		at_extrinsic: Option<u32>
	) {
		if first_write_in_tx || self.transactions.is_empty() {
			self.transactions.push(InnerValue {
				value,
				.. Default::default()
			});
		} else {
			*self.value_mut() = value;
		}

		if let Some(extrinsic) = at_extrinsic {
			self.transaction_extrinsics_mut().insert(extrinsic);
		}
	}
}

// Inserts a key into the dirty set.
//
// Returns true iff we are currently have at least one open transaction and if this
// is the first write to that transaction.
fn insert_dirty(set: &mut Vec<HashSet<StorageKey>>, key: StorageKey) -> bool {
	if let Some(dirty_keys) = set.last_mut() {
		dirty_keys.insert(key)
	} else {
		false
	}
}

impl OverlayedChangeSet {
	/// Create a new changeset with the specified transaction depth.
	pub fn with_depth(depth: usize) -> Self {
		use std::iter::repeat;
		Self {
			dirty_keys: repeat(HashSet::new()).take(depth).collect(),
			.. Default::default()
		}
	}

	/// True if no changes at all are contained in the change set.
	pub fn is_empty(&self) -> bool {
		self.changes.is_empty()
	}

	/// Get an optional reference to the value stored for the specified key.
	pub fn get(&self, key: &[u8]) -> Option<&OverlayedValue> {
		self.changes.get(key)
	}

	/// Set a new value for the specified key.
	///
	/// Can be rolled back or comitted when called inside a transaction.
	pub fn set(
		&mut self,
		key: StorageKey,
		value: Option<StorageValue>,
		at_extrinsic: Option<u32>
	) {
		let overlayed = self.changes.entry(key.clone()).or_insert_with(Default::default);
		overlayed.set(value, insert_dirty(&mut self.dirty_keys, key), at_extrinsic);
	}

	/// Get a mutable reference for a value.
	///
	/// Can be rolled back or comitted when called inside a transaction.
	#[must_use = "A change was registered, so this value MUST be modified."]
	pub fn modify(
		&mut self,
		key: StorageKey,
		init: impl Fn() -> StorageValue,
		at_extrinsic: Option<u32>
	) -> &mut Option<StorageValue> {
		let overlayed = self.changes.entry(key.clone()).or_insert_with(|| OverlayedValue {
			transactions: vec![InnerValue {
				value: Some(init()),
				extrinsics: at_extrinsic.into_iter().collect::<BTreeSet<_>>(),
			}]
		});

		// Avoid rewriting value with itself when it was already set in the current tx
		if insert_dirty(&mut self.dirty_keys, key) {
			overlayed.set(overlayed.value().cloned(), true, at_extrinsic);
		}

		overlayed.value_mut()
	}

	/// Set all values to deleted which are matched by the predicate.
	///
	/// Can be rolled back or comitted when called inside a transaction.
	pub fn clear(
		&mut self,
		predicate: impl Fn(&[u8], &OverlayedValue) -> bool,
		at_extrinsic: Option<u32>
	) {
		for (key, val) in self.changes.iter_mut().filter(|(k, v)| predicate(k, v)) {
			val.set(None, insert_dirty(&mut self.dirty_keys, key.to_owned()), at_extrinsic);
		}
	}

	/// Get a list of all changes as seen by current transaction.
	pub fn changes(&self) -> impl Iterator<Item=(&StorageKey, &OverlayedValue)> {
		self.changes.iter()
	}

	/// Get the change that is next to the supplied key.
	pub fn next_change(&self, key: &[u8]) -> Option<(&[u8], &OverlayedValue)> {
		use std::ops::Bound;
		let range = (Bound::Excluded(key), Bound::Unbounded);
		self.changes.range::<[u8], _>(range).next().map(|(k, v)| (&k[..], v))
	}

	/// Consume this changeset and return all committed changes.
	///
	/// Panics:
	/// Panics if there are open transactions: `transaction_depth() > 0`
	pub fn drain_commited(self) -> impl Iterator<Item=(StorageKey, Option<StorageValue>)> {
		assert!(self.transaction_depth() == 0);
		self.changes.into_iter().map(|(k, mut v)| (k, v.pop_transaction().value))
	}

	/// Returns the current nesting depth of the transaction stack.
	///
	/// A value of zero means that no transaction is open and changes are comitted on write.
	pub fn transaction_depth(&self) -> usize {
		self.dirty_keys.len()
	}

	/// Start a new nested transaction.
	///
	/// This allows to either commit or roll back all changes that where made while this
	/// transaction was open. Any transaction must be closed by one of the aforementioned
	/// functions before this overlay can be converted into storage changes.
	///
	/// Changes made without any open transaction are comitted immediatly.
	pub fn start_transaction(&mut self) {
		self.dirty_keys.push(Default::default());
	}

	/// Rollback the last transaction started by `start_transaction`.
	///
	/// Any changes made during that transaction are discarded.
	///
	/// Panics:
	/// Will panic if there is no open transaction.
	pub fn rollback_transaction(&mut self) {
		self.close_transaction(true);
	}

	/// Commit the last transaction started by `start_transaction`.
	///
	/// Any changes made during that transaction are committed.
	///
	/// Panics:
	/// Will panic if there is no open transaction
	pub fn commit_transaction(&mut self) {
		self.close_transaction(false);
	}

	fn close_transaction(&mut self, rollback: bool) {
		for key in self.dirty_keys.pop().expect(PROOF_DIRTY_KEYS) {
			let value = self.changes.get_mut(&key).expect(PROOF_DIRTY_OVERLAY_VALUE);

			if rollback {
				value.pop_transaction();

				// We need to remove the key as an `OverlayValue` with no transactions
				// violates its invariant of always having at least one transaction.
				if value.transactions.is_empty() {
					self.changes.remove(&key);
				}
			} else {
				let no_predecessor = if let Some(dirty_keys) = self.dirty_keys.last_mut() {
					// Not the last tx: Did the previous tx write to this key?
					dirty_keys.insert(key)
				} else {
					// Last tx: Is there already a value in the committed set?
					// Check against one rather than empty because the current tx is still
					// in the list as it is popped later in this function.
					value.transactions.len() == 1
				};

				// The previous tx or committed set holds no value for this key.
				// We just use the current tx as the previous one.
				if no_predecessor {
					return;
				}

				let dropped_tx = value.pop_transaction();
				*value.value_mut() = dropped_tx.value;
				value.transaction_extrinsics_mut().extend(dropped_tx.extrinsics);
			}
		}
	}
}
