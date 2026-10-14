use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

/// Hands a freshly built value from a control thread to the audio thread.
///
/// The control thread builds the value, which may be slow, and calls `post`.
/// The audio thread calls `take` at the top of a block. `take` never allocates,
/// locks, or frees. The audio thread's old copy is dropped as a plain reference
/// decrement, and the control thread frees it later, once it holds the last reference.
pub struct Handoff<T> {
    pending: AtomicPtr<T>,
    kept: Mutex<Vec<Arc<T>>>,
}

impl<T> Default for Handoff<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Handoff<T> {
    pub fn new() -> Self {
        Handoff {
            pending: AtomicPtr::new(ptr::null_mut()),
            kept: Mutex::new(Vec::new()),
        }
    }

    /// Control thread only.
    pub fn post(&self, value: Arc<T>) {
        let raw = Arc::into_raw(value.clone()) as *mut T;
        let old = self.pending.swap(raw, Ordering::AcqRel);
        if !old.is_null() {
            // The audio thread never saw this one. Its reference comes back here.
            drop(unsafe { Arc::from_raw(old as *const T) });
        }
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.push(value);
        let n = kept.len();
        if n > 2 {
            let mut i = 0;
            kept.retain(|a| {
                i += 1;
                i > n - 2 || Arc::strong_count(a) > 1
            });
        }
    }

    /// Audio thread only. Returns the newest posted value, once.
    pub fn take(&self) -> Option<Arc<T>> {
        let p = self.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        if p.is_null() {
            None
        } else {
            Some(unsafe { Arc::from_raw(p as *const T) })
        }
    }

    pub fn kept_len(&self) -> usize {
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl<T> Drop for Handoff<T> {
    fn drop(&mut self) {
        let p = *self.pending.get_mut();
        if !p.is_null() {
            drop(unsafe { Arc::from_raw(p as *const T) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    #[test]
    fn take_returns_the_newest_value_once() {
        let h = Handoff::new();
        assert!(h.take().is_none());
        h.post(Arc::new(1));
        h.post(Arc::new(2));
        assert_eq!(*h.take().unwrap(), 2);
        assert!(h.take().is_none());
    }

    #[test]
    fn unconsumed_values_are_freed_by_the_control_side() {
        let h = Handoff::new();
        for i in 0..100 {
            h.post(Arc::new(i));
        }
        assert!(h.kept_len() <= 3, "kept {}", h.kept_len());
    }

    #[test]
    fn old_values_are_freed_after_the_audio_side_moves_on() {
        let h = Handoff::new();
        h.post(Arc::new(0));
        let mut current = h.take().unwrap();
        for i in 1..50 {
            h.post(Arc::new(i));
            if let Some(n) = h.take() {
                current = n;
            }
        }
        assert_eq!(*current, 49);
        assert!(h.kept_len() <= 3, "kept {}", h.kept_len());
    }

    #[test]
    fn a_reader_on_another_thread_only_moves_forward() {
        let h = Arc::new(Handoff::new());
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(AtomicUsize::new(0));
        let reader = {
            let (h, stop, seen) = (h.clone(), stop.clone(), seen.clone());
            std::thread::spawn(move || {
                let mut last = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    if let Some(v) = h.take() {
                        assert!(*v >= last, "went backwards: {} after {}", *v, last);
                        last = *v;
                        seen.store(last, Ordering::Relaxed);
                    }
                }
                if let Some(v) = h.take() {
                    assert!(*v >= last);
                    seen.store(*v, Ordering::Relaxed);
                }
            })
        };
        for i in 1..=5000usize {
            h.post(Arc::new(i));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), 5000);
        assert!(h.kept_len() <= 3);
    }
}
