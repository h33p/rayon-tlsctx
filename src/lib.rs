use std::sync::Mutex;
use std::cell::Cell;
use std::ops::{DerefMut, Deref};

/// A thread local storage container for Rayon jobs
///
/// This context can be used to efficiently clone `inner`, only when it's necessary.
pub struct ThreadLocalCtx<T, F> {
    inner: Mutex<F>,
    init_mutex: Mutex<Vec<(Option<T>, bool)>>,
    cloned: Cell<*mut (Option<T>, bool)>
}

unsafe impl<T: Send, F: Send> Sync for ThreadLocalCtx<T, F> {}
unsafe impl<T: Send, F: Send> Send for ThreadLocalCtx<T, F> {}

impl<T, F: Fn() -> T> ThreadLocalCtx<T, F> {
    /// Create a new `TlsCtx`
    pub fn new(inner: F) -> Self {
        Self {
            inner: Mutex::new(inner),
            init_mutex: Mutex::new(vec![]),
            cloned: Cell::new(std::ptr::null_mut())
        }
    }

    /// Get a thread local context reference with dynamically checked borrow rules
    ///
    /// # Safety
    ///
    /// It is entirely up to you to ensure that this scope runs only on one thread pool at a given
    /// time.
    ///
    /// # Panics
    ///
    /// If called from outside of a Rayon thread pool, or when the reference is already being
    /// borrowed.
    pub unsafe fn get<'a>(&'a self) -> ThreadLocalMut<'a, T, F> {
        if self.cloned.get().is_null() {
            let mut data = self.init_mutex.lock().unwrap();
            if self.cloned.get().is_null() {
                *data = (0..rayon::current_num_threads()).map(|_| (None, false)).collect();
                self.cloned.set(data.as_mut_ptr());
            }
        }

        let tid = rayon::current_thread_index().unwrap();

        match &mut *self.cloned.get().add(tid) {
            (_, true) => panic!("Already borrowed the value on thread {}!", tid),
            (Some(val), b) => {
                *b = true;
                ThreadLocalMut {
                    val,
                    parent: self,
                    tid
                }
            },
            (val, b) => {
                *b = true;
                let cloned = (self.inner.lock().unwrap())();
                *val = Some(cloned);
                ThreadLocalMut {
                    val: val.as_mut().unwrap(),
                    parent: self,
                    tid
                }
            }
        }
    }
}

pub struct ThreadLocalMut<'a, T, F> {
    val: &'a mut T,
    parent: &'a ThreadLocalCtx<T, F>,
    tid: usize
}

impl<'a, T, F> Deref for ThreadLocalMut<'a, T, F> {
    type Target = T;

    fn deref(&self) -> &T {
        self.val
    }
}

impl<'a, T, F> DerefMut for ThreadLocalMut<'a, T, F> {
    fn deref_mut(&mut self) -> &mut T {
        self.val
    }
}

impl<'a, T, F> Drop for ThreadLocalMut<'a, T, F> {
    fn drop(&mut self) {
        unsafe {
            (*self.parent.cloned.get().add(self.tid)).1 = false;
        }
    }
}
