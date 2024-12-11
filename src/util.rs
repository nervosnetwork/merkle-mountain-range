use crate::collections::BTreeMap;
use crate::{MMRStoreReadOps, MMRStoreWriteOps, Result, MMR};
use core::cell::RefCell;

#[derive(Clone)]
pub struct MemStore<T>(RefCell<BTreeMap<u64, T>>);

impl<T> Default for MemStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> MemStore<T> {
    fn new() -> Self {
        MemStore(RefCell::new(Default::default()))
    }
}

impl<T: Clone> MMRStoreReadOps<T> for &MemStore<T> {
    fn get(&self, pos: u64) -> Result<Option<T>> {
        Ok(self.0.borrow().get(&pos).cloned())
    }
}

impl<T> MMRStoreWriteOps<T> for &MemStore<T> {
    fn insert(&mut self, pos: u64, elem: T) -> Result<()> {
        self.0.borrow_mut().insert(pos, elem);
        Ok(())
    }
}

pub type MemMMR<'a, T, M> = MMR<T, M, &'a MemStore<T>>;
