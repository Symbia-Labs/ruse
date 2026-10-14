//! The transport's clock. Wow and flutter are a pure function of time, so every
//! track of a machine, and every instance given the same clock, drifts together.

use std::f64::consts::PI;

#[inline(always)]
fn hash(i: i64, seed: u64) -> f64 {
    let mut z = (i as u64).wrapping_add(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// Smooth noise in -1..1, new values `rate` times a second.
#[inline(always)]
fn noise(t: f64, rate: f64, seed: u64) -> f64 {
    let u = t * rate;
    let i = u.floor();
    let f = u - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash(i as i64, seed);
    let b = hash(i as i64 + 1, seed);
    a + (b - a) * s
}

/// How far the tape is from where it should be, in seconds, at time t.
/// `wow` and `flutter` are peak speed deviations as fractions.
pub fn excursion(t: f64, wow: f64, flutter: f64, capstan: f64) -> f64 {
    let tau = 2.0 * PI;
    let mut e = 0.0;
    if wow > 0.0 {
        e -= wow * 0.6 * (tau * 0.55 * t).cos() / (tau * 0.55);
        e -= wow * 0.3 * (tau * 1.37 * t + 1.1).cos() / (tau * 1.37);
        e += wow * 0.1 * noise(t, 0.25, 11) / (tau * 0.125);
    }
    if flutter > 0.0 {
        e -= flutter * 0.55 * (tau * capstan * t + 0.4).cos() / (tau * capstan);
        e -= flutter * 0.25 * (tau * 2.63 * capstan * t + 2.0).cos() / (tau * 2.63 * capstan);
        e += flutter * 0.2 * noise(t, 70.0, 29) / (tau * 35.0);
    }
    e
}

/// The largest the excursion can get, seconds.
pub fn reach(wow: f64, flutter: f64, capstan: f64) -> f64 {
    let tau = 2.0 * PI;
    wow * (0.6 / (tau * 0.55) + 0.3 / (tau * 1.37) + 0.1 / (tau * 0.125))
        + flutter * (0.55 / (tau * capstan) + 0.25 / (tau * 2.63 * capstan) + 0.2 / (tau * 35.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_time_gives_the_same_excursion() {
        let a = excursion(123.456, 0.002, 0.001, 11.6);
        let b = excursion(123.456, 0.002, 0.001, 11.6);
        assert_eq!(a, b);
    }

    #[test]
    fn the_excursion_stays_inside_its_reach() {
        let r = reach(0.002, 0.001, 11.6);
        for k in 0..20000 {
            let e = excursion(k as f64 * 0.013, 0.002, 0.001, 11.6);
            assert!(e.abs() <= r * 1.0001, "{} > {}", e.abs(), r);
        }
    }
}
