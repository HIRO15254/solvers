use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    thread::LocalKey,
};

/// Owned scratch returned to the current thread's pool on drop. A pool can
/// lend several buffers at once during nested Rayon jobs; no TLS borrow is
/// held while computing or entering another parallel loop.
pub(crate) struct Reused<T: 'static> {
    value: Option<T>,
    pool: Option<&'static LocalKey<RefCell<Vec<T>>>>,
}

impl<T: Default> Reused<T> {
    pub(crate) fn take(pool: &'static LocalKey<RefCell<Vec<T>>>) -> Self {
        Self::take_with(pool, T::default)
    }

    pub(crate) fn empty() -> Self {
        Self {
            value: Some(T::default()),
            pool: None,
        }
    }
}

impl<T: Default + Clone> Clone for Reused<T> {
    fn clone(&self) -> Self {
        let mut copy = self.pool.map_or_else(Self::empty, Self::take);
        copy.deref_mut().clone_from(self);
        copy
    }
}

impl<T> Reused<T> {
    pub(crate) fn take_with(
        pool: &'static LocalKey<RefCell<Vec<T>>>,
        create: impl FnOnce() -> T,
    ) -> Self {
        Self {
            value: Some(pool.with(|p| p.borrow_mut().pop()).unwrap_or_else(create)),
            pool: Some(pool),
        }
    }
}

impl<T> Deref for Reused<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<T> DerefMut for Reused<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

/// Values a thread's pool keeps. A value taken on one worker can be dropped on
/// another, so without a bound one thread's pool could keep growing while the
/// others allocate.
const POOL_LIMIT: usize = 64;

impl<T> Drop for Reused<T> {
    fn drop(&mut self) {
        if let Some(pool) = self.pool {
            // A value may outlive its originating worker or be dropped during
            // TLS teardown, when there is no pool left to retain it.
            let _ = pool.try_with(|p| {
                let mut p = p.borrow_mut();
                if p.len() < POOL_LIMIT {
                    p.push(self.value.take().unwrap());
                }
            });
        }
    }
}
