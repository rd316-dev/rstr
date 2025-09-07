pub struct Lru<K, V> {
    max_entries: usize,
    counter: u32,
    entries: Vec<LruEntry<K, V>>
}

struct LruEntry<K, V> {
    last_accessed: u32,
    key: K,
    value: V
}

impl <K, V> Lru<K, V> where K: PartialEq {
    pub fn new(max_entries: usize) -> Self {
        Lru { max_entries: max_entries, counter: 0, entries: Vec::with_capacity(max_entries) }
    }

    pub fn get(&mut self, key: K) -> Option<&V> {
        let entry = self.entries.iter_mut().find(|e| e.key == key)?;

        let current_counter = self.counter;
        self.counter += 1;

        entry.last_accessed = current_counter;

        Some(&entry.value)
    }

    pub fn put(&mut self, key: K, value: V) {
        let current_counter = self.counter;
        self.counter += 1;

        let entry = LruEntry { last_accessed: current_counter, key: key, value: value };
        let index: usize;

        if self.entries.len() >= self.max_entries {
            let mut min_value = u32::MAX;
            let mut min_index = 0;

            for i in 0..self.entries.len() {
                let current = &self.entries[i];
                if current.last_accessed < min_value {
                    min_value = current.last_accessed;
                    min_index = i;
                }
            }

            index = min_index;
        } else {
            index = self.entries.len()
        }

        self.entries[index] = entry;
    }

}