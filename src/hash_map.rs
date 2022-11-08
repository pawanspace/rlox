#[derive(Debug, Clone)]
pub(crate) enum Entry<T> {
    Occupied(String, T),
    Vacant
} 


#[derive(Debug)]
pub(crate) struct Table<T: Clone> {
    entries: Vec<Entry<T>>,
    capacity: usize,
    size: usize,
    load_factor: usize
}

impl <T: Clone> Table<T> {
    pub(crate) fn init(capacity: usize) -> Table<T> {
        let mut entries: Vec<Entry<T>> = Vec::with_capacity(capacity);
        entries.fill(Entry::Vacant);
        Table {
            entries,
            capacity,
            size: 0,
            load_factor: 70
        }
    }

    pub(crate) fn insert(&mut self, key: String, value: T) -> bool {
        self.ensure_capacity();
        let bucket = self.find_bucket(key);

        false
    }

    fn ensure_capacity(&mut self) {
        if (self.size/self.capacity)*100 > self.load_factor {
            self.entries.resize(self.capacity*2, Entry::Vacant);
        }
    }


    fn find_bucket(&self, key: String) -> usize {
        let h = self.hash(key);
        let mut bucket = h % (self.capacity as u32);
        
        while self.is_occupied(bucket, key) {
            bucket = (bucket + 1) % (self.capacity as u32);
        }

        bucket as usize
    }

    fn is_occupied(&self, bucket: u32, key: String) -> bool {
        match self.entries[bucket as usize] {
            Entry::Occupied(existing,_) => {
                // if key is same we will use the same index
                if existing == key {
                    false
                } else {
                    true
                } 
            },
            Entry::Vacant => false
        }
    } 

    //fnv hash impl basic
    fn hash(&self, key: String) -> u32 {
        let mut hash = 2166136261;
        let chars: Vec<char> = key.chars().collect();
        for i in 0..key.len() {
            hash ^= chars[i] as u32;
            hash *= 16777619;
        }
        hash
    }

}


