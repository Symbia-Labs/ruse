use std::sync::OnceLock;

pub const LANGEVIN_COUNT: usize = 4096;
pub const LANGEVIN_TOP: f32 = 16.0;

/// L(x) = coth(x) - 1/x and its slope, for x >= 0, tabulated. L is odd and L' even.
pub struct Langevin {
    pub l: Vec<f32>,
    pub d: Vec<f32>,
    pub scale: f32,
}

static LANGEVIN: OnceLock<Langevin> = OnceLock::new();

/// Built once. Call it off the audio thread before the first block.
pub fn langevin() -> &'static Langevin {
    LANGEVIN.get_or_init(|| {
        let scale = (LANGEVIN_COUNT - 1) as f32 / LANGEVIN_TOP;
        let mut l = vec![0.0f32; LANGEVIN_COUNT + 1];
        let mut d = vec![0.0f32; LANGEVIN_COUNT + 1];
        for i in 0..=LANGEVIN_COUNT {
            let x = i as f64 / scale as f64;
            if x < 1e-3 {
                l[i] = (x / 3.0 - x * x * x / 45.0) as f32;
                d[i] = (1.0 / 3.0 - x * x / 15.0) as f32;
            } else {
                let s = 1.0 / x.sinh();
                l[i] = (1.0 / x.tanh() - 1.0 / x) as f32;
                d[i] = (1.0 / (x * x) - s * s) as f32;
            }
        }
        Langevin { l, d, scale }
    })
}

/// Magnetization M in units of saturation, field H in units of the knee.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hysteresis {
    pub m: f32,
    pub h: f32,
    pub dh: f32,
}

impl Hysteresis {
    /// dM/dt for a state, by the Jiles-Atherton equation.
    // The negated comparison is deliberate: a NaN must fall into the asymptote branch.
    #[inline(always)]
    #[allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord)]
    pub fn slope(m: f32, h: f32, dh: f32, k: f32, c: f32, alpha: f32, lt: &[f32], dt: &[f32], scale: f32) -> f32 {
        let q = h + alpha * m;
        let ax = q.abs();
        let mut lan;
        let lan_slope;
        // Past the table, the asymptote. Written so that anything not a number
        // lands here too and never reaches the table index.
        if !(ax < LANGEVIN_TOP - 0.01) {
            lan = 1.0 - 1.0 / ax;
            lan_slope = 1.0 / (ax * ax);
        } else {
            let p = ax * scale;
            let i = p as usize;
            let f = p - i as f32;
            lan = lt[i] + f * (lt[i + 1] - lt[i]);
            lan_slope = dt[i] + f * (dt[i + 1] - dt[i]);
        }
        if q < 0.0 {
            lan = -lan;
        }
        let diff = lan - m;
        let dir: f32 = if dh >= 0.0 { 1.0 } else { -1.0 };
        let mut irreversible = 0.0f32;
        if diff * dir > 0.0 {
            // The pinning term, held away from zero so the slope stays finite and positive.
            let pin = (1.0 - c) * k;
            let den = (pin - alpha * diff * dir).max(0.25 * pin);
            irreversible = (1.0 - c) * diff * dir / den;
        }
        let reversible = c * lan_slope;
        dh * (irreversible + reversible) / (1.0 - alpha * reversible)
    }

    /// Advance one sample of field, `t` seconds on (midpoint Runge-Kutta).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, h_new: f32, t: f32, k: f32, c: f32, alpha: f32, lt: &[f32], dt: &[f32], scale: f32) -> f32 {
        let dh_new = (h_new - self.h) / t;
        let k1 = t * Self::slope(self.m, self.h, self.dh, k, c, alpha, lt, dt, scale);
        let k2 = t * Self::slope(
            self.m + 0.5 * k1,
            0.5 * (self.h + h_new),
            0.5 * (self.dh + dh_new),
            k,
            c,
            alpha,
            lt,
            dt,
            scale,
        );
        let mut next = self.m + k2;
        if !(next > -1.0 && next < 1.0) {
            next = if next.is_finite() { next.clamp(-1.0, 1.0) } else { 0.0 };
        }
        self.m = next;
        self.h = h_new;
        self.dh = dh_new;
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_function() {
        let t = langevin();
        let x = 2.0f64;
        let i = (x * t.scale as f64) as usize;
        let exact = 1.0 / x.tanh() - 1.0 / x;
        assert!((t.l[i] as f64 - exact).abs() < 2e-3);
    }

    #[test]
    fn the_loop_saturates_and_stays_bounded() {
        let t = langevin();
        let mut h = Hysteresis::default();
        let mut peak = 0.0f32;
        for n in 0..4000 {
            let field = 8.0 * (n as f32 * 0.05).sin();
            let m = h.step(field, 1.0 / 88200.0, 0.5, 0.96, 0.02, &t.l, &t.d, t.scale);
            assert!(m.is_finite() && m.abs() <= 1.0);
            peak = peak.max(m.abs());
        }
        assert!(peak > 0.8, "peak {}", peak);
    }
}
