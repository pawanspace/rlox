use crate::common::FatPointer;
use crate::memory;
use std::borrow::BorrowMut;
use std::fmt::Debug;
#[derive(Debug, Clone)]
pub(crate) enum Entry<T> {
    Occupied(FatPointer, T),
    Vacant,
    TombStone,
}

#[derive(Debug)]
pub(crate) struct Table<T>
where
    T: Debug,
    T: Clone,
{
    entries: Vec<Entry<T>>,
    capacity: usize,
    size: usize,
    load_factor: usize,
}

impl<T> Table<T>
where
    T: Clone,
    T: Debug,
{
    pub(crate) fn init(capacity: usize) -> Table<T> {
        let mut entries: Vec<Entry<T>> = vec![];
        entries.resize(capacity, Entry::Vacant);
        Table {
            entries,
            capacity,
            size: 0,
            load_factor: 70,
        }
    }

    pub(crate) fn insert(&mut self, key: FatPointer, value: T) -> bool {
        self.ensure_capacity();
        let bucket = self.find_bucket(&key, &self.entries);
        let new_value = matches!(&self.entries[bucket], Entry::Occupied(_, _));
        self.entries[bucket] = Entry::Occupied(key, value);
        self.size += 1;
        new_value
    }

    pub(crate) fn get(&self, key: FatPointer) -> Option<&T> {
        let entry = self.find_entry(&key).unwrap();
        match entry {
            Entry::Occupied(value, data) => Some(data),
            _ => None,
        }
    }

    pub(crate) fn get_mut(&mut self, key: FatPointer) -> Option<&mut T> {
        let entry = self.find_entry_mut(&key).unwrap();
        match entry {
            Entry::Occupied(value, data) => Some(data),
            _ => None,
        }
    }

    pub(crate) fn delete(&mut self, key: FatPointer) -> Option<T> {
        let bucket = self.find_bucket(&key, &self.entries);
        let value = self.get_at_index(bucket);
        if value.is_some() {
            self.insert_tombstone(bucket);
        }
        value
    }

    fn insert_tombstone(&mut self, bucket: usize) {
        self.entries[bucket] = Entry::TombStone;
    }

    fn get_at_index(&mut self, bucket: usize) -> Option<T> {
        let entry = &self.entries[bucket];
        return match entry {
            Entry::Occupied(_, value) => Some(value.clone()),
            _ => None,
        };
    }

    fn ensure_capacity(&mut self) {
        if ((self.size + 1) / self.capacity) * 100 > self.load_factor {
            self.capacity = (self.capacity * 2) + 1;
            let mut temp_entries: Vec<Entry<T>> = vec![];
            temp_entries.resize(self.capacity, Entry::Vacant);
            self.size = 0;
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

    fn find_bucket(&self, key: &FatPointer, entries: &Vec<Entry<T>>) -> usize {
        let mut bucket = key.hash % (self.capacity as u32);

        while self.is_occupied(bucket, key, entries) {
            bucket = (bucket + 1) % (self.capacity as u32);
        }

        bucket as usize
    }

    pub(crate) fn dump(&self) {
        println!("{:?}", self.entries);
    }

    pub(crate) fn find_entry_with_value(&self, str_value: &str, hash: u32) -> Option<&FatPointer> {
        let mut bucket = hash % (self.capacity as u32);
        loop {
            return match &self.entries[bucket as usize] {
                Entry::Occupied(existing, _) => {
                    // if key is same we will use the same index
                    if memory::read_string(existing.ptr, existing.size).eq(str_value) {
                        Some(&existing)
                    } else {
                        bucket = (bucket + 1) % (self.capacity as u32);
                        continue;
                    }
                }
                Entry::Vacant => None,
                Entry::TombStone => {
                    bucket = (bucket + 1) % (self.capacity as u32);
                    continue;
                }
            };
        }
    }

    pub(crate) fn find_entry(&self, key: &FatPointer) -> Option<&Entry<T>> {
        let index = self.find_entry_index(key);
        println!("Entry index: {:?}", index);
        return match index {
            Some(index) => self.entries.get(index),
            None => None,
        };
    }

    fn find_entry_mut(&mut self, key: &FatPointer) -> Option<&mut Entry<T>> {
        let index = self.find_entry_index(key);
        return match index {
            Some(index) => self.entries.get_mut(index),
            None => None,
        };
    }

    fn find_entry_index(&self, key: &FatPointer) -> Option<usize> {
        let mut bucket = key.hash % (self.capacity as u32);
        loop {
            let entry = self.entries.get(bucket as usize);
            return match entry {
                Some(entry) => match entry {
                    Entry::Occupied(existing, _) => {
                        if existing.eq(key) {
                            return Some(bucket as usize);
                        } else {
                            bucket = (bucket + 1) % (self.capacity as u32);
                            continue;
                        }
                    },
                    Entry::Vacant => None,
                    Entry::TombStone => {
                        bucket = (bucket + 1) % (self.capacity as u32);
                        continue;
                    }
                },
                None => None,
            };
        }
    }

    fn is_occupied(&self, bucket: u32, key: &FatPointer, entries: &Vec<Entry<T>>) -> bool {
        match &entries[bucket as usize] {
            Entry::Occupied(existing, _) => {
                // if key is same we will use the same index
                if memory::eq(existing.ptr, key.ptr) {
                    false
                } else {
                    true
                }
            }
            Entry::Vacant | Entry::TombStone => false,
        }
    }
}

#[derive(Debug, Clone)]
struct TestValue {
    id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hasher::hash;

    fn create_fat_ptr(value: &mut &str) -> FatPointer {
        FatPointer {
            ptr: value.to_string().as_mut_ptr(),
            size: value.len(),
            hash: hash(value),
        }
    }

    #[test]
    fn can_hold_multiple_keys() {
        let mut map = Table::init(2);
        let one = create_fat_ptr(&mut "one");
        let two = create_fat_ptr(&mut "two");

        map.insert(one, true);
        map.insert(two, true);
        assert!(map.size == 2);
    }

    #[test]
    fn can_hold_multiple_keys_multiple_tables() {
        let mut map = Table::init(2);
        let one = create_fat_ptr(&mut "one");
        let two = create_fat_ptr(&mut "two");

        let mut map2: Table<bool> = Table::init(2);
        let one2 = create_fat_ptr(&mut "one");
        let two2 = create_fat_ptr(&mut "two");

        map.insert(one.clone(), true);
        map.insert(two, true);
        assert!(map.size == 2);

        map2.insert(one2.clone(), true);
        map2.insert(two2, true);
        assert!(map2.size == 2);
        assert_eq!(map2.get(one2.clone()), Some(&true));
        assert_eq!(map.get(one.clone()), Some(&true));
    }

    #[test]
    fn can_hold_and_return_multiple_keys() {
        let mut map = Table::init(2);
        let one = create_fat_ptr(&mut "one");
        let two = create_fat_ptr(&mut "two");

        map.insert(one.clone(), true);
        map.insert(two.clone(), false);

        assert_eq!(map.get(one.clone()), Some(&true));
        assert_eq!(map.get(two.clone()), Some(&false));
    }

    #[test]
    fn can_hold_and_delete_multiple_keys() {
        let mut map = Table::init(2);
        let one = create_fat_ptr(&mut "one");
        let two = create_fat_ptr(&mut "two");

        map.insert(one.clone(), true);
        map.insert(two.clone(), false);

        map.delete(one.clone());
        assert_eq!(map.get(one.clone()), None);
    }

    #[test]
    fn can_expand_capacity_as_required() {
        let mut map = Table::init(1);
        let one = create_fat_ptr(&mut "one");
        let two = create_fat_ptr(&mut "two");
        let _three = create_fat_ptr(&mut "three");

        map.insert(one.clone(), true);
        assert_eq!(map.capacity, 3);

        map.insert(two.clone(), false);
        assert_eq!(map.capacity, 3);

        map.insert(two.clone(), true);
        assert_eq!(map.capacity, 7);
    }

    #[test]
    fn can_handle_reference() {
        let mut map = Table::init(1);
        let value = TestValue { id: 1 };
        let one = create_fat_ptr(&mut "one");
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
}
