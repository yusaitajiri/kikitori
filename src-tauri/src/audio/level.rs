//! Peak level per 100 ms in dBFS (section 7). The UI maps −60..0 dBFS onto the bar.

pub const FLOOR_DBFS: f32 = -60.0;

pub fn to_dbfs(amplitude: f32) -> f32 {
    if amplitude <= 1e-6 { FLOOR_DBFS } else { (20.0 * amplitude.log10()).clamp(FLOOR_DBFS, 0.0) }
}

/// RMS level of a clip in dBFS (used by the hallucination filter).
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return FLOOR_DBFS;
    }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq <= 1e-12 { -120.0 } else { 10.0 * mean_sq.log10() }
}

/// Accumulates samples and yields one peak reading per window.
pub struct PeakMeter {
    window: usize,
    count: usize,
    peak: f32,
}

impl PeakMeter {
    /// `window` in samples, e.g. 1600 for 100 ms at 16 kHz.
    pub fn new(window: usize) -> Self {
        Self { window: window.max(1), count: 0, peak: 0.0 }
    }

    /// Returns the readings (dBFS) completed by this block.
    pub fn push(&mut self, samples: &[f32], mut on_reading: impl FnMut(f32)) {
        for &s in samples {
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
            self.count += 1;
            if self.count == self.window {
                on_reading(to_dbfs(self.peak));
                self.count = 0;
                self.peak = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbfs_conversion() {
        assert_eq!(to_dbfs(1.0), 0.0);
        assert!((to_dbfs(0.5) + 6.02).abs() < 0.01);
        assert_eq!(to_dbfs(0.0), FLOOR_DBFS);
        assert_eq!(to_dbfs(2.0), 0.0);
    }

    #[test]
    fn one_reading_per_window() {
        let mut m = PeakMeter::new(4);
        let mut readings = Vec::new();
        m.push(&[0.1, -0.5, 0.2, 0.0, 0.25, 0.0], |r| readings.push(r));
        assert_eq!(readings.len(), 1);
        assert!((readings[0] - to_dbfs(0.5)).abs() < 1e-6);
        m.push(&[0.0, 0.0], |r| readings.push(r));
        assert_eq!(readings.len(), 2);
        assert!((readings[1] - to_dbfs(0.25)).abs() < 1e-6);
    }

    #[test]
    fn rms_of_full_scale_sine_is_about_minus_three() {
        let s: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.1).sin()).collect();
        assert!((rms_dbfs(&s) + 3.0).abs() < 0.2);
    }
}
