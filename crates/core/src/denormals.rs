//! Flush-to-zero for the audio thread.
//!
//! A filter or feedback path that decays toward silence can land in the subnormal range, where
//! arithmetic is far slower on many processors and a host may flag the output. Format wrappers hold a
//! [`FlushToZero`] guard while they run a plugin's `process`, so subnormal results become exact zeros
//! and the previous floating-point mode comes back when the guard drops.

#[cfg(target_arch = "aarch64")]
mod mode {
    use std::arch::asm;

    /// FPCR bit 24, flush-to-zero.
    pub const FLUSH: usize = 1 << 24;

    pub fn get() -> usize {
        let v: u64;
        // Reading FPCR touches no memory and no other state.
        unsafe { asm!("mrs {v}, fpcr", v = out(reg) v, options(nomem, nostack, preserves_flags)) };
        v as usize
    }

    pub fn set(v: usize) {
        // FPCR holds rounding and exception settings for the current thread only.
        unsafe { asm!("msr fpcr, {v}", v = in(reg) v as u64, options(nomem, nostack)) };
    }
}

#[cfg(target_arch = "x86_64")]
mod mode {
    use std::arch::asm;

    /// MXCSR bit 15 (flush to zero) and bit 6 (denormals are zero).
    pub const FLUSH: usize = (1 << 15) | (1 << 6);

    pub fn get() -> usize {
        let mut v: u32 = 0;
        unsafe { asm!("stmxcsr [{p}]", p = in(reg) &mut v, options(nostack, preserves_flags)) };
        v as usize
    }

    pub fn set(v: usize) {
        let v = v as u32;
        unsafe { asm!("ldmxcsr [{p}]", p = in(reg) &v, options(nostack, readonly)) };
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
mod mode {
    pub const FLUSH: usize = 0;
    pub fn get() -> usize {
        0
    }
    pub fn set(_v: usize) {}
}

/// While this value lives, subnormal floating-point results on the current thread become zero.
pub struct FlushToZero {
    saved: usize,
}

impl FlushToZero {
    #[inline]
    pub fn enable() -> FlushToZero {
        let saved = mode::get();
        mode::set(saved | mode::FLUSH);
        FlushToZero { saved }
    }
}

impl Drop for FlushToZero {
    #[inline]
    fn drop(&mut self) {
        mode::set(self.saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    fn tiny_product() -> f32 {
        // 1e-38 is just above the smallest normal f32, so halving it lands in the subnormal range.
        black_box(1e-38f32) * black_box(0.1f32)
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    #[test]
    fn a_guard_turns_subnormal_results_into_zero_and_restores_the_mode() {
        let outside = tiny_product();
        assert!(outside != 0.0 && outside.is_subnormal(), "{outside:e}");
        {
            let _guard = FlushToZero::enable();
            assert_eq!(tiny_product(), 0.0);
        }
        assert!(tiny_product().is_subnormal(), "the mode came back");
    }

    #[test]
    fn guards_nest() {
        let before = mode::get();
        {
            let _a = FlushToZero::enable();
            let _b = FlushToZero::enable();
        }
        assert_eq!(mode::get(), before);
    }
}
