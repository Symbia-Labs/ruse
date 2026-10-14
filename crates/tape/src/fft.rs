/// In-place radix-2 complex FFT, unnormalized. Forward uses e^(-i...), inverse e^(+i...).
pub fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);

    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_direct_dft() {
        let n = 16;
        let xr: Vec<f64> = (0..n).map(|i| ((i * 7 + 3) % 11) as f64 - 5.0).collect();
        let xi: Vec<f64> = (0..n).map(|i| ((i * 5 + 1) % 7) as f64 - 3.0).collect();
        let (mut re, mut im) = (xr.clone(), xi.clone());
        fft(&mut re, &mut im, false);
        for k in 0..n {
            let (mut sr, mut si) = (0.0, 0.0);
            for t in 0..n {
                let a = -2.0 * std::f64::consts::PI * (k * t) as f64 / n as f64;
                sr += xr[t] * a.cos() - xi[t] * a.sin();
                si += xr[t] * a.sin() + xi[t] * a.cos();
            }
            assert!((re[k] - sr).abs() < 1e-9 && (im[k] - si).abs() < 1e-9, "bin {}", k);
        }
    }

    #[test]
    fn inverse_undoes_forward() {
        let n = 1024;
        let xr: Vec<f64> = (0..n).map(|i| (i as f64 * 0.37).sin()).collect();
        let (mut re, mut im) = (xr.clone(), vec![0.0; n]);
        fft(&mut re, &mut im, false);
        fft(&mut re, &mut im, true);
        for i in 0..n {
            assert!((re[i] / n as f64 - xr[i]).abs() < 1e-12);
        }
    }
}
