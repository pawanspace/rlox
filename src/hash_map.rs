use std::fmt::Debug;

#[derive(Debug, Clone)]
pub(crate) enum Entry<T> {
    Occupied(String, T),
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

    pub(crate) fn insert(&mut self, key: &str, value: T) -> bool {
        self.ensure_capacity();
        let bucket = self.find_bucket(key, &self.entries);
        let new_value = matches!(&self.entries[bucket], Entry::Occupied(_, _));
        self.entries[bucket] = Entry::Occupied(key.to_string(), value);
        self.size += 1;
        new_value
    }

    pub(crate) fn get(&mut self, key: &str) -> Option<&T> {
        self.find_entry(key)
    }

    pub(crate) fn delete(&mut self, key: &str) -> Option<T> {
        let bucket = self.find_bucket(key, &self.entries);
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
                        temp_entries[bucket] = Entry::Occupied(key.to_string(), value.clone());
                        self.size += 1;
                    }
                    _ => (),
                }
            }

            self.entries = temp_entries;
            println!("Ensured capacity {:?}", self.entries.len());
        }
    }

    fn find_bucket(&self, key: &str, entries: &Vec<Entry<T>>) -> usize {
        let h = self.hash(key);
        let mut bucket = h % (self.capacity as u32);

        while self.is_occupied(bucket, key, entries) {
            bucket = (bucket + 1) % (self.capacity as u32);
        }

        bucket as usize
    }

    pub(crate) fn dump(&mut self) {
        println!("{:?}", self.entries);
    }

    fn find_entry(&self, key: &str) -> Option<&T> {
        let h = self.hash(key);
        let mut bucket = h % (self.capacity as u32);
        loop {
            return match &self.entries[bucket as usize] {
                Entry::Occupied(existing, value) => {
                    // if key is same we will use the same index
                    if existing == key {
                        Some(&value)
                    } else {
                        bucket = (bucket + 1) % (self.capacity as u32);
                        continue;
                    }
                }
                Entry::Vacant => None,
                Entry::TombStone => {
                    bucket = (bucket + 1) % (self.capacity as u32);
                    continue;
                },
            };
        }
    }

    fn is_occupied(&self, bucket: u32, key: &str, entries: &Vec<Entry<T>>) -> bool {
        match &entries[bucket as usize] {
            Entry::Occupied(existing, _) => {
                // if key is same we will use the same index
                if existing == key {
                    false
                } else {
                    true
                }
            }
            Entry::Vacant | Entry::TombStone => false,
        }
    }

    //fnv hash impl basic
    fn hash(&self, key: &str) -> u32 {
        let mut hash = 2166136261;
        let chars: Vec<char> = key.chars().collect();
        for i in 0..key.len() {
            hash ^= chars[i] as u32;
            hash = hash.wrapping_mul(16777619);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_hold_multiple_keys() {
        let mut map = Table::init(2);
        map.insert("one", true);
        map.insert("two", true);
        assert!(map.size == 2);
    }

    #[test]
    fn can_hold_and_return_multiple_keys() {
        let mut map = Table::init(2);
        map.insert("one", true);
        map.insert("two", false);

        assert_eq!(map.get("one"), Some(&true));
        assert_eq!(map.get("two"), Some(&false));
    }

    #[test]
    fn can_hold_and_delete_multiple_keys() {
        let mut map = Table::init(2);
        map.insert("one", true);
        map.insert("two", false);
        assert_eq!(map.get("one"), Some(&true));

        map.delete("one");
        assert_eq!(map.get("one"), None);
    }

    #[test]
    fn can_expand_capacity_as_required() {
        let mut map = Table::init(1);
        map.insert("one", Some(&true));
        assert_eq!(map.capacity, 3);
        map.insert("two", Some(&false));
        assert_eq!(map.capacity, 3);
        map.insert("three", Some(&false));
        assert_eq!(map.capacity, 7);
    }
}
