//! # Clone only when it's necessary
//!
//! This library provides an efficient way to clone values in a rayon thread pool, but only once
//! per thread. It cuts down on computation time for potentially expensive cloning operations.
//!
//!
//! # Examples
//!
//! ```
//! use rayon_tlsctx::ThreadLocalCtx;
//! use rayon::iter::*;
//!
//! const NUM_COPIES: usize = 16;
//!
//! let mut buf: Vec<u16> = (0..!0).collect();
//!
//! // Create a thread local context with value 0.
//! let ctx = ThreadLocalCtx::new(|| {
//!     // Simulate expensive operation.
//!     // Since we are building unlocked context,
//!     // the sleeps will occur concurrently.
//!     std::thread::sleep_ms(200);
//!     0
//! });
//!
//! let pool = rayon::ThreadPoolBuilder::new().num_threads(64).build().unwrap();
//!
//! // Run inside a custom thread pool.
//! pool.install(|| {
//!     // Sum the buffer `NUM_COPIES` times and accumulate the results
//!     // into the threaded pool of counts. Note that the counts may be
//!     // Unevenly distributed.
//!     (0..NUM_COPIES)
//!         .into_par_iter()
//!         .flat_map(|_| buf.par_iter())
//!         .for_each(|i| {
//!             let mut cnt = unsafe { ctx.get() };
//!             *cnt += *i as usize;
//!         });
//! });
//!
//!
//! let buf_sum = buf.into_iter().fold(0, |acc, i| acc + i as usize);
//!
//! // What matters is that the final sum matches the expected value.
//! assert_eq!(ctx.into_iter().sum::<usize>(), buf_sum * NUM_COPIES);
//! ```

use std::cell::Cell;
use std::iter::FilterMap;
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;

/// A thread local storage container for Rayon jobs
///
/// This context can be used to efficiently clone `inner`, only when it's necessary.
pub struct ThreadLocalCtx<T, F> {
    inner: F,
    init_mutex: Mutex<Vec<(Option<T>, bool)>>,
    cloned: Cell<*mut (Option<T>, bool)>,
}

unsafe impl<T, F: Send + Sync> Sync for ThreadLocalCtx<T, F> {}
unsafe impl<T, F: Send + Sync> Send for ThreadLocalCtx<T, F> {}

impl<T, F: Fn() -> T> ThreadLocalCtx<T, F> {
    /// Create a new `TlsCtx`
    ///
    /// # Examples
    ///
    /// Creating a thread-local byte buffer:
    /// ```
    /// use rayon_tlsctx::ThreadLocalCtx;
    /// let ctx = ThreadLocalCtx::new(Vec::<u8>::new);
    /// ```
    pub fn new(inner: F) -> Self {
        Self {
            inner, //: Mutex::new(inner),
            init_mutex: Mutex::new(vec![]),
            cloned: Cell::new(std::ptr::null_mut()),
        }
    }

    /// Create a new `TlsCtx`.
    ///
    /// This context utilises a lock for cloning values, making it usable for non-sync types.
    ///
    /// Cloning an initialised buffer for each thread:
    /// ```
    /// use rayon_tlsctx::ThreadLocalCtx;
    /// use rayon::iter::*;
    /// use std::cell::Cell;
    /// # use rand::prelude::*;
    /// # let mut rng = rand::thread_rng();
    ///
    /// let mut buf: Vec<u16> = (0..!0).collect();
    /// buf.shuffle(&mut rng);
    ///
    /// let buf = (buf, Cell::new(0));
    ///
    /// // Must use new_locked, because cloning a cell across threads is not allowed
    /// let ctx = ThreadLocalCtx::new_locked(move || buf.clone());
    ///
    /// (0..16).into_par_iter().for_each(|_| unsafe { ctx.get(); });
    /// ```
    pub fn new_locked(inner: F) -> ThreadLocalCtx<T, impl Fn() -> T> {
        let locked_inner = Mutex::new(inner);

        let inner = move || (locked_inner.lock().unwrap())();

        ThreadLocalCtx {
            inner,
            init_mutex: Mutex::new(vec![]),
            cloned: Cell::new(std::ptr::null_mut()),
        }
    }

    /// Get a thread local context reference with dynamically checked borrow rules
    ///
    /// # Safety
    ///
    /// Only one thread pool at a given time should use this scope. The thread pool size can not
    /// grow midway through.
    ///
    /// # Panics
    ///
    /// When the reference is already being borrowed.
    ///
    /// # Examples
    ///
    /// ```
    /// use rayon_tlsctx::ThreadLocalCtx;
    /// use rayon::iter::*;
    ///
    /// const NUM_COPIES: usize = 16;
    ///
    /// let mut buf: Vec<u16> = (0..!0).collect();
    ///
    /// // Create a thread local context with value 0.
    /// let ctx = ThreadLocalCtx::new(|| 0);
    ///
    /// // Sum the buffer `NUM_COPIES` times and accumulate the results
    /// // into the threaded pool of counts. Note that the counts may be
    /// // Unevenly distributed.
    /// (0..NUM_COPIES)
    ///     .into_par_iter()
    ///     .flat_map(|_| buf.par_iter())
    ///     .for_each(|i| {
    ///         let mut cnt = unsafe { ctx.get() };
    ///         *cnt += *i as usize;
    ///     });
    ///
    /// let buf_sum = buf.into_iter().fold(0, |acc, i| acc + i as usize);
    ///
    /// // What matters is that the final sum matches the expected value.
    /// assert_eq!(ctx.into_iter().sum::<usize>(), buf_sum * NUM_COPIES);
    /// ```
    pub unsafe fn get(&self) -> ThreadLocalMut<T, F> {
        if self.cloned.get().is_null() {
            let mut data = self.init_mutex.lock().unwrap();
            if self.cloned.get().is_null() {
                *data = (0..=rayon::current_num_threads())
                    .map(|_| (None, false))
                    .collect();

                self.cloned.set(data.as_mut_ptr());
            }
        }

        let tid = rayon::current_thread_index().map(|i| i + 1).unwrap_or(0);

        match &mut *self.cloned.get().add(tid) {
            (_, true) => panic!("Already borrowed the value on thread {}!", tid),
            (Some(val), b) => {
                *b = true;
                ThreadLocalMut {
                    val,
                    parent: self,
                    tid,
                }
            }
            (val, b) => {
                *b = true;
                let cloned = (self.inner)();
                *val = Some(cloned);
                ThreadLocalMut {
                    val: val.as_mut().unwrap(),
                    parent: self,
                    tid,
                }
            }
        }
    }
}

type VecIter<T> = std::vec::IntoIter<(Option<T>, bool)>;
type FmapFn<T> = fn((Option<T>, bool)) -> Option<T>;

/// Consume the context and retrieve all created items.
impl<T, F> IntoIterator for ThreadLocalCtx<T, F> {
    type Item = T;
    type IntoIter = FilterMap<VecIter<T>, FmapFn<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.init_mutex
            .into_inner()
            .unwrap()
            .into_iter()
            .filter_map(|(i, _)| i)
    }
}

/// Borrowed thread local variable.
///
/// This structure tracks borrow rules at runtime, it may be necessary to manually
/// drop the object, if multiple rayon loops are involved.
pub struct ThreadLocalMut<'a, T, F> {
    val: &'a mut T,
    parent: &'a ThreadLocalCtx<T, F>,
    tid: usize,
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
