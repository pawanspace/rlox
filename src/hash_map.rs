//! An open-addressing hash table (`Table<T>`), plus its entry states.
//!
//! ## What a hash table is for here
//! The VM needs fast key→value lookup keyed by *strings*: global variables by
//! name, and a set of *interned* strings (so identical string literals are
//! stored once). This table provides that. Keys are `FatPointer`s (a raw
//! pointer + length + cached hash from `common.rs`); values are the generic
//! `T`.
//!
//! ## Open addressing vs. chaining (the core idea)
//! There are two classic ways to handle two keys hashing to the same bucket
//! ("collisions"):
//!   - *Chaining*: each bucket holds a linked list of entries. Simple, but
//!     costs a pointer-chase and an allocation per node.
//!   - *Open addressing* (used here): every entry lives directly in the bucket
//!     array. On a collision you *probe* — walk to another slot — until you
//!     find the key or an empty slot. No per-entry allocation, cache-friendly.
//! This table probes *linearly*: on collision it tries the next bucket,
//! `(i + 1) % capacity`, wrapping around the end of the array. `% capacity`
//! is what makes the walk circle back to the start instead of running off the
//! end. clox uses this same open-addressing + linear-probing design.
//!
//! ## Why deletion needs "tombstones"
//! With linear probing, a lookup stops as soon as it hits a truly empty
//! (`Vacant`) slot — that empty slot proves the key can't be further along the
//! probe chain. But if you *delete* an entry by blanking it to `Vacant`, you
//! might cut a probe chain in half and make later keys unreachable. The fix is
//! a `TombStone`: a "was occupied, now deleted" marker that a lookup treats as
//! "keep probing" (not "stop"), while an insert may reuse it. That is why the
//! `Entry` enum has three states, not two.
//!
//! ## Load factor and resizing
//! As a hash table fills, probe chains get longer and lookups slow down. So
//! once the fraction of used slots ("load factor") crosses a threshold, the
//! table grows and re-inserts everything into a bigger array, shortening the
//! chains. See `ensure_capacity`, which resizes at the `load_factor` threshold
//! via `(size + 1) * 100 > capacity * load_factor`.

use crate::common::FatPointer;
use crate::memory;
use std::borrow::BorrowMut;
use std::fmt::Debug;

/// The state of a single bucket in the table.
///
/// The three-way split is what makes safe deletion possible under linear
/// probing (see the module docs on tombstones).
#[derive(Debug, Clone)]
pub(crate) enum Entry<T> {
    /// Holds a live key/value pair.
    Occupied(FatPointer, T),
    /// Never used — a probe that reaches here stops (the key is not present).
    Vacant,
    /// Previously occupied, then deleted — a probe must skip past it and keep
    /// looking; an insert may overwrite it.
    TombStone,
}

/// A hash map from `FatPointer` (string) keys to `T` values, using open
/// addressing with linear probing.
#[derive(Debug)]
pub(crate) struct Table<T>
where
    T: Debug,
    T: Clone,
{
    /// The bucket array. Its length is always `capacity`; each slot is one
    /// `Entry` (Occupied / Vacant / TombStone).
    entries: Vec<Entry<T>>,
    /// Number of buckets (the length of `entries`). Kept as a separate field
    /// and used as the modulus for probe wraparound.
    capacity: usize,
    /// Number of live entries. Incremented only when `insert` adds a new key
    /// (not on overwrite) and decremented by `delete`, so it tracks the true
    /// count of occupied slots.
    size: usize,
    /// Resize threshold as a percentage (70 = grow at ~70% full). Applied by
    /// `ensure_capacity` via `(size + 1) * 100 > capacity * load_factor`.
    load_factor: usize,
}

impl<T> Table<T>
where
    T: Clone,
    T: Debug,
{
    /// Create an empty table with `capacity` buckets, all `Vacant`.
    pub(crate) fn init(capacity: usize) -> Table<T> {
        let mut entries: Vec<Entry<T>> = vec![];
        // Pre-fill the whole bucket array so it can be indexed directly by
        // bucket number; open addressing needs the slots to exist up front.
        entries.resize(capacity, Entry::Vacant);
        Table {
            entries,
            capacity,
            size: 0,
            load_factor: 70,
        }
    }

    /// Insert `key`/`value`. Returns whether the chosen bucket was already
    /// `Occupied` (i.e. this was an overwrite of an existing key).
    pub(crate) fn insert(&mut self, key: FatPointer, value: T) -> bool {
        self.ensure_capacity();
        let bucket = self.find_bucket(&key, &self.entries);
        // `new_value` is true when the slot was already Occupied — i.e. this
        // insert is an *overwrite* of an existing key, not a new entry.
        let new_value = matches!(&self.entries[bucket], Entry::Occupied(_, _));
        self.entries[bucket] = Entry::Occupied(key, value);
        // Only count a genuinely new key toward `size`. Overwrites replace an
        // existing entry and add nothing, so they must not increment `size`
        // (doing so would drift the count high and trigger premature resizes).
        if !new_value {
            self.size += 1;
        }
        new_value
    }

    /// Look up `key`; return `Some(&value)` if present, else `None`.
    pub(crate) fn get(&self, key: FatPointer) -> Option<&T> {
        // Match on the `Option` from `find_entry`: `Some(Occupied)` is a hit,
        // anything else (a `None`, or a non-Occupied slot) is a miss returning
        // `None`. A missing key must not panic.
        let entry = self.find_entry(&key);
        match entry {
            Some(Entry::Occupied(value, data)) => Some(data),
            _ => None,
        }
    }

    /// Like `get`, but returns a mutable reference to the stored value.
    pub(crate) fn get_mut(&mut self, key: FatPointer) -> Option<&mut T> {
        let entry = self.find_entry_mut(&key);
        match entry {
            Some(Entry::Occupied(value, data)) => Some(data),
            _ => None,
        }
    }

    /// Delete `key`, returning its old value if it was present.
    ///
    /// Deletion replaces the slot with a `TombStone` (not `Vacant`) so probe
    /// chains through this bucket stay intact — see the module docs.
    pub(crate) fn delete(&mut self, key: FatPointer) -> Option<T> {
        let bucket = self.find_bucket(&key, &self.entries);
        let value = self.get_at_index(bucket);
        if value.is_some() {
            self.insert_tombstone(bucket);
            self.size -= 1;
        }
        value
    }

    /// Overwrite bucket `bucket` with a `TombStone` marker.
    fn insert_tombstone(&mut self, bucket: usize) {
        self.entries[bucket] = Entry::TombStone;
    }

    /// Read (clone out) the value at `bucket`, if that slot is `Occupied`.
    fn get_at_index(&mut self, bucket: usize) -> Option<T> {
        let entry = &self.entries[bucket];
        return match entry {
            Entry::Occupied(_, value) => Some(value.clone()),
            _ => None,
        };
    }

    /// Grow and rehash the table when it gets too full.
    ///
    /// Growing means: allocate a bigger bucket array, then re-insert every live
    /// entry (their bucket index depends on `capacity`, so it changes when the
    /// capacity changes — everything must be re-placed).
    fn ensure_capacity(&mut self) {
        // Resize once the projected load crosses `load_factor` percent. The
        // comparison cross-multiplies — `(size + 1) * 100 > capacity * load_factor`
        // — instead of dividing, so it never truncates. (An earlier version wrote
        // `((size + 1) / capacity) * 100 > load_factor`, where the integer
        // division collapsed to 0 until the table was essentially full, ignoring
        // the 70% target entirely.)
        if (self.size + 1)  * 100 > self.load_factor * self.capacity {
            self.capacity = (self.capacity * 2) + 1;
            let mut temp_entries: Vec<Entry<T>> = vec![];
            temp_entries.resize(self.capacity, Entry::Vacant);
            self.size = 0;
            // Re-insert every occupied entry into the new, larger array.
            // Tombstones and vacants are dropped in the process (a nice
            // side-effect of rehashing: it clears out deletion markers).
            for entry in self.entries.iter() {
                match entry {
                    Entry::Occupied(key, value) => {
                        let bucket = self.find_bucket(key, &temp_entries);
                        temp_entries[bucket] = Entry::Occupied(key.clone(), value.clone());
                        self.size += 1;
                    }
                    _ => (),
                }
            }

            self.entries = temp_entries;
        }
    }

    /// Find the bucket index where `key` should live: start at
    /// `hash % capacity`, then probe forward while the slot is "occupied by a
    /// *different* key" until a usable slot is found.
    fn find_bucket(&self, key: &FatPointer, entries: &Vec<Entry<T>>) -> usize {
        let mut bucket = key.hash % (self.capacity as u32);

        // BUG: this loop only stops when `is_occupied` returns false — i.e. at a
        // Vacant/TombStone slot, or the same key. If the table were completely
        // full of *different* keys with no empty slot, this would loop forever.
        // In practice `ensure_capacity` (mis)fires just before full, so it is
        // usually avoided — but it is not robust.
        while self.is_occupied(bucket, key, entries) {
            // `+ 1` moves to the next slot; `% capacity` wraps back to 0 at the
            // end so the probe is circular.
            bucket = (bucket + 1) % (self.capacity as u32);
        }

        bucket as usize
    }

    /// Debug helper: print the whole bucket array.
    pub(crate) fn dump(&self) {
        println!("{:?}", self.entries);
    }

    /// Interning lookup: find an existing key whose *string content* equals
    /// `str_value`, matching by decoding the stored bytes.
    ///
    /// This is used during compilation to reuse an already-stored copy of a
    /// string instead of allocating a new one. It matches by comparing the
    /// actual characters (via `read_string`), which is different from how
    /// `find_entry_index` matches (by `FatPointer` equality) — see the NOTE on
    /// `is_occupied`.
    pub(crate) fn find_entry_with_value(&self, str_value: &str, hash: u32) -> Option<&FatPointer> {
        let mut bucket = hash % (self.capacity as u32);
        loop {
            return match &self.entries[bucket as usize] {
                Entry::Occupied(existing, _) => {
                    // Same *content* => reuse this stored string.
                    if memory::read_string(existing.ptr, existing.size).eq(str_value) {
                        Some(&existing)
                    } else {
                        // Collision with a different string: probe forward.
                        bucket = (bucket + 1) % (self.capacity as u32);
                        continue;
                    }
                }
                // A truly empty slot proves the string isn't stored: stop.
                Entry::Vacant => None,
                // Deleted marker: the string may still be further along.
                Entry::TombStone => {
                    bucket = (bucket + 1) % (self.capacity as u32);
                    continue;
                }
            };
        }
    }

    /// Return a shared reference to the `Entry` for `key`, if found.
    pub(crate) fn find_entry(&self, key: &FatPointer) -> Option<&Entry<T>> {
        let index = self.find_entry_index(key);
        println!("Entry index: {:?}", index); // stray debug output
        return match index {
            Some(index) => self.entries.get(index),
            None => None,
        };
    }

    /// Mutable-reference version of `find_entry`.
    fn find_entry_mut(&mut self, key: &FatPointer) -> Option<&mut Entry<T>> {
        let index = self.find_entry_index(key);
        return match index {
            Some(index) => self.entries.get_mut(index),
            None => None,
        };
    }

    /// Core lookup: probe from `hash % capacity` and return the bucket index of
    /// the entry matching `key`, or `None` if a `Vacant` slot is reached first.
    fn find_entry_index(&self, key: &FatPointer) -> Option<usize> {
        let mut bucket = key.hash % (self.capacity as u32);
        loop {
            let entry = self.entries.get(bucket as usize);
            return match entry {
                Some(entry) => match entry {
                    Entry::Occupied(existing, _) => {
                        // Matches by `FatPointer` equality (see common.rs: same
                        // ptr + size + hash). Contrast with `is_occupied`, which
                        // compares only the raw pointer address, and with
                        // `find_entry_with_value`, which compares string bytes.
                        if existing.eq(key) {
                            return Some(bucket as usize);
                        } else {
                            bucket = (bucket + 1) % (self.capacity as u32);
                            continue;
                        }
                    },
                    // Empty slot: key definitively absent, stop probing.
                    Entry::Vacant => None,
                    // Deleted: skip and keep probing.
                    Entry::TombStone => {
                        bucket = (bucket + 1) % (self.capacity as u32);
                        continue;
                    }
                },
                None => None,
            };
        }
    }

    /// Decide whether `find_bucket` should keep probing past `bucket`.
    ///
    /// Returns `true` (keep going) only when the slot holds a *different* key.
    /// A slot holding the same key, or an empty/tombstone slot, returns `false`
    /// so `find_bucket` stops there.
    fn is_occupied(&self, bucket: u32, key: &FatPointer, entries: &Vec<Entry<T>>) -> bool {
        match &entries[bucket as usize] {
            Entry::Occupied(existing, _) => {
                // Match by pointer identity — the same notion of "same key" that
                // `find_entry_index` (via `FatPointer::eq`) uses, valid because
                // every string is interned to one canonical pointer.
                if existing.ptr.eq(&key.ptr) {
                    false
                } else {
                    true
                }
            }
            // BUG (gap #1): treating a tombstone as a stopping point means the
            // insert probe stops here instead of scanning on to see whether the
            // key already exists further along — so re-inserting after a delete
            // can create a duplicate. See HASHMAP.md gap #1 /
            // reinsert_after_delete_does_not_duplicate.
            Entry::Vacant | Entry::TombStone => false,
        }
    }
}

/// A tiny value type used only by the unit tests below.
#[derive(Debug, Clone)]
struct TestValue {
    id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hasher::hash;

    fn create_fat_ptr(value: &mut String) -> FatPointer {
        FatPointer {
            ptr: value.as_mut_ptr(),
            size: value.len(),
            hash: hash(value),
        }
    }

    // Two distinct keys can coexist in the table (basic collision-free insert).
    #[test]
    fn can_hold_multiple_keys() {
        let mut map = Table::init(2);
        let (mut one_s, mut two_s) = (String::from("one"), String::from("two"));
        let one = create_fat_ptr(&mut one_s);
        let two = create_fat_ptr(&mut two_s);

        map.insert(one, true);
        map.insert(two, true);
        assert!(map.size == 2);
    }

    // Two separate tables are independent — entries in one don't leak into the
    // other, and each can be read back correctly.
    #[test]
    fn can_hold_multiple_keys_multiple_tables() {
        let mut map = Table::init(2);
        let (mut one_s, mut two_s) = (String::from("one"), String::from("two"));
        let one = create_fat_ptr(&mut one_s);
        let two = create_fat_ptr(&mut two_s);

        let mut map2: Table<bool> = Table::init(2);
        let (mut one2_s, mut two2_s) = (String::from("one"), String::from("two"));
        let one2 = create_fat_ptr(&mut one2_s);
        let two2 = create_fat_ptr(&mut two2_s);

        map.insert(one.clone(), true);
        map.insert(two, true);
        assert!(map.size == 2);

        map2.insert(one2.clone(), true);
        map2.insert(two2, true);
        assert!(map2.size == 2);
        assert_eq!(map2.get(one2.clone()), Some(&true));
        assert_eq!(map.get(one.clone()), Some(&true));
    }

    // Values are retrievable by key and preserved distinctly (true vs false).
    #[test]
    fn can_hold_and_return_multiple_keys() {
        let mut map = Table::init(2);
        let (mut one_s, mut two_s) = (String::from("one"), String::from("two"));
        let one = create_fat_ptr(&mut one_s);
        let two = create_fat_ptr(&mut two_s);

        map.insert(one.clone(), true);
        map.insert(two.clone(), false);

        assert_eq!(map.get(one.clone()), Some(&true));
        assert_eq!(map.get(two.clone()), Some(&false));
    }

    // After deletion the key reads back as absent (tombstone path works for
    // lookup).
    #[test]
    fn can_hold_and_delete_multiple_keys() {
        let mut map = Table::init(2);
        let (mut one_s, mut two_s) = (String::from("one"), String::from("two"));
        let one = create_fat_ptr(&mut one_s);
        let two = create_fat_ptr(&mut two_s);

        map.insert(one.clone(), true);
        map.insert(two.clone(), false);

        map.delete(one.clone());
        assert_eq!(map.get(one.clone()), None);
    }

    // Demonstrates automatic growth: capacity jumps (1 -> 3 -> 7 via the
    // `capacity * 2 + 1` rule in ensure_capacity) as the table fills.
    #[test]
    fn can_expand_capacity_as_required() {
        let mut map = Table::init(1);
        let (mut one_s, mut two_s, mut three_s) =
            (String::from("one"), String::from("two"), String::from("three"));
        let one = create_fat_ptr(&mut one_s);
        let two = create_fat_ptr(&mut two_s);
        let _three = create_fat_ptr(&mut three_s);

        map.insert(one.clone(), true);
        assert_eq!(map.capacity, 3);

        map.insert(two.clone(), false);
        assert_eq!(map.capacity, 3);

        map.insert(two.clone(), true);
        assert_eq!(map.capacity, 7);
    }

    // `get_mut` hands out a mutable reference to the stored value, so mutations
    // through it persist in the table.
    #[test]
    fn can_handle_reference() {
        let mut map = Table::init(1);
        let value = TestValue { id: 1 };
        let mut one_s = String::from("one");
        let one = create_fat_ptr(&mut one_s);
        {
            map.insert(one.clone(), value);
            let existing = map.get_mut(one.clone());
            existing.unwrap().id = 2;
        }
        assert!(
            map.get_mut(one.clone()).unwrap().id.eq(&2),
            "Expected value to update based on reference."
        );
    }

    #[test]
    fn insert_overwrite_keeps_correct_size() {
        let mut map = Table::init(2);
        let mut one_s = String::from("one");
        let one = create_fat_ptr(&mut one_s);
        map.insert(one.clone(), 1);
        map.insert(one, 2);
        assert_eq!(map.size, 1);
    }

    #[test]
    fn get_probes_past_tombstone() {
        let mut map = Table::init(8);
        let mut a_s = String::from("a");
        let mut b_s = String::from("b");
        let a = FatPointer {ptr: a_s.as_mut_ptr(), size: a_s.len(), hash: 0 };
        let b = FatPointer {ptr: b_s.as_mut_ptr(), size: b_s.len(), hash: 0 };

        map.insert(a.clone(), 1);
        map.insert(b.clone(), 2);
        map.delete(a);

        assert_eq!(map.get(b), Some(&2));
    }

    // Open bug: `find_bucket` stops at the first tombstone, so re-inserting a key
    // whose slot was tombstoned writes a duplicate instead of finding the existing
    // entry further along the probe chain (see tasks.md / HASHMAP.md gap #2).
    // This test currently FAILS on purpose until `find_bucket`'s insert probe is
    // fixed to remember the first tombstone but keep scanning for a match.
    #[test]
    fn reinsert_after_delete_does_not_duplicate() {
        let mut map = Table::init(8);
        let mut a_s = String::from("a");
        let mut b_s = String::from("b");
        let a = FatPointer {ptr: a_s.as_mut_ptr(), size: a_s.len(), hash: 0 };
        let b = FatPointer {ptr: b_s.as_mut_ptr(), size: b_s.len(), hash: 0 };

        map.insert(a.clone(), 1);
        map.insert(b.clone(), 2);
        map.delete(a);
        assert_eq!(map.size, 1);
        map.insert(b.clone(), 3);
        assert_eq!(map.size, 1);
    }

    #[test]
    fn find_entry_mut_should_not_panic_for_missing_key() {
        let mut map = Table::init(8);
        let (mut one_s, mut missing_s) = (String::from("one"), String::from("missing"));
        let one = create_fat_ptr(&mut one_s);
        let missing = create_fat_ptr(&mut missing_s);
        map.insert(one.clone(), 1);
        assert!(map.get_mut(missing).is_none());
        assert!(map.get_mut(one).is_some());
    }

    #[test]
    fn resizes_when_load_factor_exceeded() {
        let mut map = Table::init(10);
        for i in 0..8 {
            let mut key = format!("key-{}", i);
            let k = FatPointer{ptr: key.as_mut_ptr(), size: key.len(), hash: i as u32};
            map.insert(k.clone(), i);
        }

        assert!(map.capacity > 10);
    }
}
