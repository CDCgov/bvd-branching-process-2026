use super::hazard_rates::HazardRateFn;
use crate::validation::ensure_non_negative;
use anyhow::Result;

pub struct FixedRateFn {
    // A rate of infection in terms of people per unit time
    r: f64,
    t_end: f64,
}

impl FixedRateFn {
    /// # Errors
    /// - The rate of infection must be non-negative.
    pub fn new(r: f64, t_end: f64) -> Result<Self> {
        ensure_non_negative("rate of infection", r)?;
        ensure_non_negative("end time", t_end)?;
        Ok(Self { r, t_end })
    }
}

impl HazardRateFn for FixedRateFn {
    fn rate(&self, _t: f64) -> f64 {
        self.r
    }
    fn cum_rate(&self, t: f64) -> f64 {
        self.r * t
    }
    fn inverse_cum_rate(&self, events: f64) -> Option<f64> {
        let t = events / self.r;
        (t <= self.t_end).then_some(t)
    }
    fn duration(&self) -> f64 {
        self.t_end
    }
}

#[cfg(test)]
mod test {
    use super::{FixedRateFn, HazardRateFn};

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_fixed_rate_fn() {
        let rate = FixedRateFn::new(0.5, 5.0).unwrap();
        assert_eq!(rate.rate(5.0), 0.5);
        assert_eq!(rate.cum_rate(5.0), 2.5);
        assert_eq!(rate.inverse_cum_rate(2.5), Some(5.0));
        assert!(rate.inverse_cum_rate(rate.duration() + 1.0).is_none());
    }
}
