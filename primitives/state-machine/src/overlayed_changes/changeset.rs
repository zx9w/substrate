use super::{StorageKey, StorageValue};

use itertools::Itertools;
use std::collections::{HashSet, BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone)]
#[cfg_attr(test, derive(PartialEq))]
struct InnerValue {
	/// Current value. None if value has been deleted. One value per open nested transaction.
	value: Option<StorageValue>,
	/// The set of extrinsic indices where the values has been changed.
	/// Is filled only if runtime has announced changes trie support.
	extrinsics: BTreeSet<u32>,
}

/// The storage value, used inside OverlayedChanges.
#[derive(Debug, Default, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct OverlayedValue {
	transactions: Vec<InnerValue>,
}

#[derive(Debug, Default, Clone)]
pub struct OverlayedChangeSet {
	/// Stores the actual changes.
	changes: BTreeMap<StorageKey, OverlayedValue>,
	/// Stores which keys are dirty per transaction. Needed in order to determine which
	/// values to merge into the parent transaction on commit.
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
	/// The most recent value contained in this overlay.
	pub fn value(&self) -> Option<&StorageValue> {
		self.transactions.last()
			.expect("A StorageValue is always initialized with one value.\
			The last element is never removed as those are committed changes.")
			.value
			.as_ref()
	}

	/// List of indices of extrinsics which modified the value using this overlay.
	pub fn extrinsics(&self) -> impl Iterator<Item=&u32> {
		self.transactions.iter().flat_map(|t| t.extrinsics.iter()).unique()
	}

	pub fn value_mut(&mut self) -> &mut Option<StorageValue> {
		&mut self.transactions.last_mut()
			.expect("A StorageValue is always initialized with one value.\
			The last element is never removed as those are committed changes.")
			.value
	}

	fn tx_extrinsics_mut(&mut self) -> &mut BTreeSet<u32> {
		&mut self.transactions.last_mut().expect("").extrinsics
	}

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
			self.tx_extrinsics_mut().insert(extrinsic);
		}
	}
}

fn insert_dirty(set: &mut Vec<HashSet<StorageKey>>, key: StorageKey) -> bool {
	if let Some(dirty_keys) = set.last_mut() {
		dirty_keys.insert(key)
	} else {
		false
	}
}

impl OverlayedChangeSet {
	pub fn with_depth(depth: usize) -> Self {
		use std::iter::repeat;
		Self {
			dirty_keys: repeat(HashSet::new()).take(depth).collect(),
			.. Default::default()
		}
	}

	pub fn is_empty(&self) -> bool {
		self.changes.is_empty()
	}

	pub fn get(&self, key: &[u8]) -> Option<&OverlayedValue> {
		self.changes.get(key)
	}

	pub fn set(
		&mut self,
		key: StorageKey,
		value: Option<StorageValue>,
		at_extrinsic: Option<u32>
	) -> &mut OverlayedValue
	{
		let first_write_in_tx = insert_dirty(&mut self.dirty_keys, key.clone());
		let overlayed = self.changes.entry(key).or_insert_with(Default::default);
		overlayed.set(value, first_write_in_tx, at_extrinsic);
		overlayed
	}

	pub fn changes(&self) -> impl Iterator<Item=(&StorageKey, &OverlayedValue)> {
		self.changes.iter()
	}

	pub fn next_key(&self, key: &[u8]) -> Option<(&[u8], &OverlayedValue)> {
		use std::ops::Bound;
		let range = (Bound::Excluded(key), Bound::Unbounded);
		self.changes.range::<[u8], _>(range).next().map(|(k, v)| (&k[..], v))
	}

	pub fn clear(
		&mut self,
		predicate: impl Fn(&[u8], &OverlayedValue) -> bool,
		at_extrinsic: Option<u32>
	)
	{
		for (key, val) in self.changes.iter_mut() {
			if ! predicate(key, val) {
				continue;
			}

			let first_write_in_tx = insert_dirty(&mut self.dirty_keys, key.to_owned());
			val.set(None, first_write_in_tx, at_extrinsic);
		}
	}

	pub fn drain_commited(self) -> impl Iterator<Item=(StorageKey, Option<StorageValue>)> {
		assert!(self.transaction_depth() == 0);
		self.changes
			.into_iter()
			.map(|(k, mut v)| (k, v.transactions.pop().expect("Always at least one value").value))
	}

	/// Returns the current nesting depth of the transaction stack.
	pub fn transaction_depth(&self) -> usize {
		self.dirty_keys.len()
	}

	pub fn start_transaction(&mut self) {
		self.dirty_keys.push(Default::default());
	}

	pub fn rollback_transaction(&mut self) {
		for key in self.dirty_keys.pop().expect("Transactions must be balanced.") {
			let value = self.changes.get_mut(&key).expect("Key was marked as dirty.");
			value.transactions.pop();

			// We need to remove the key as an `OverlayValue` with no contents
			// violates its invariant of always having at least one value.
			if value.transactions.is_empty() {
				self.changes.remove(&key);
			}
		}
	}

	pub fn commit_transaction(&mut self) {
		for key in self.dirty_keys.pop().expect("Transactions must be balanced.") {
			let value = self.changes.get_mut(&key).expect("Key was marked as dirty.");
			let merge_tx = ! if let Some(dirty_keys) = self.dirty_keys.last_mut() {
				// Not the last tx: Did the previous tx write to this key?
				dirty_keys.insert(key)
			} else {
				// Last tx: Is there already a value in the committed set?
				// Check against one rather than empty because the current tx is still
				// in the list as it is popped later in this function.
				value.transactions.len() == 1
			};

			// No need to merge if the previous tx has never written to this key.
			// We just use the current tx as the previous one.
			if ! merge_tx {
				return;
			}

			let dropped_tx = value.transactions.pop().expect("Key was marked dirty for this tx");
			*value.value_mut() = dropped_tx.value;
			value.tx_extrinsics_mut().extend(dropped_tx.extrinsics);
		}
	}
}
