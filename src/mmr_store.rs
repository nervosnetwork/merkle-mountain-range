use crate::{vec::Vec, BTreeMap, Result};

#[derive(Default)]
pub struct MMRBatch<Elem, Store> {
    memory_batch: BTreeMap<u64, Elem>,
    store: Store,
}

impl<Elem, Store> MMRBatch<Elem, Store> {
    pub fn new(store: Store) -> Self {
        MMRBatch {
            memory_batch: BTreeMap::new(),
            store,
        }
    }

    pub fn append(&mut self, pos: u64, elems: Vec<Elem>) {
        for (i, elem) in elems.into_iter().enumerate() {
            self.insert(pos + i as u64, elem);
        }
    }

    pub fn insert(&mut self, pos: u64, elem: Elem) {
        self.memory_batch.insert(pos, elem);
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}

impl<Elem: Clone, Store: MMRStoreReadOps<Elem>> MMRBatch<Elem, Store> {
    pub fn get_elem(&self, pos: u64) -> Result<Option<Elem>> {
        if let Some(elem) = self.memory_batch.get(&pos) {
            Ok(Some(elem.clone()))
        } else {
            self.store.get(pos)
        }
    }
}

impl<Elem, Store: MMRStoreWriteOps<Elem>> MMRBatch<Elem, Store> {
    pub fn commit(&mut self) -> Result<()> {
        let batch = core::mem::take(&mut self.memory_batch);

        for (pos, elem) in batch {
            self.store.insert(pos, elem)?;
        }
        Ok(())
    }
}

pub trait MMRStoreReadOps<Elem> {
    fn get(&self, pos: u64) -> Result<Option<Elem>>;
}

pub trait MMRStoreWriteOps<Elem> {
    fn insert(&mut self, pos: u64, elem: Elem) -> Result<()>;
}
