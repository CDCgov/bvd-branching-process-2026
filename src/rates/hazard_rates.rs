pub trait HazardRateFn {
    /// Returns the rate of infection at time t.
    fn rate(&self, t: f64) -> f64;

    /// Returns the cumulative rate of infection at time t.
    fn cum_rate(&self, t: f64) -> f64;

    /// Returns the inverse cumulative rate of infection for a given number of events.
    fn inverse_cum_rate(&self, events: f64) -> Option<f64>;

    #[allow(dead_code)]
    /// Returns the duration of the rate function.
    fn duration(&self) -> f64;
}

/// A utility for scaling and shifting an infectiousness rate function
pub struct ScaledRateFn<'a, T>
where
    T: HazardRateFn + ?Sized,
{
    pub base: &'a T,
    pub scale: f64,
    pub elapsed: f64,
}

impl<'a, T: ?Sized + HazardRateFn> ScaledRateFn<'a, T> {
    #[must_use]
    pub fn new(base: &'a T, scale: f64, elapsed: f64) -> Self {
        Self {
            base,
            scale,
            elapsed,
        }
    }
}

impl<T: ?Sized + HazardRateFn> HazardRateFn for ScaledRateFn<'_, T> {
    /// Returns the rate of infection at time `t` scaled by a factor of `self.scale`,
    /// and shifted by `self.elapsed`.
    fn rate(&self, t: f64) -> f64 {
        self.base.rate(t + self.elapsed) * self.scale
    }
    /// Returns the cumulative rate for a time interval starting at `self.elapsed`, scaled by a factor
    /// of `self.scale`. For example, say you want to calculate the
    /// interval from 3.0 -> 4.0; you would create a `ScaledRateFn` with an elapsed of 3.0 and
    /// take `cum_rate(1.0)` (the end of the period - the start).
    fn cum_rate(&self, t: f64) -> f64 {
        (self.base.cum_rate(t + self.elapsed) - self.base.cum_rate(self.elapsed)) * self.scale
    }
    /// Returns the expected time, starting at `self.elapsed` by which an expected number of infection
    /// `events` will occur, and sped up by a factor of `self.scale`.
    /// For example, say the current time is 2.1 and you want to calculate the time to infect the
    /// next person (events=1.0). You would create a `ScaledRateFn` with an elapsed of 2.1 and take
    /// `inverse_cum_rate(1.0)`. If you want to increase the rate by a factor of 2.0 (halve the
    /// expected time to infect that person), you would create a `ScaledRateFn` with a scale of 2.0.
    fn inverse_cum_rate(&self, events: f64) -> Option<f64> {
        let elapsed_cum_rate = self.base.cum_rate(self.elapsed);
        Some(
            self.base
                .inverse_cum_rate(events / self.scale + elapsed_cum_rate)?
                - self.elapsed,
        )
    }

    /// Returns the duration of the rate function.
    fn duration(&self) -> f64 {
        self.base.duration() - self.elapsed
    }
}

#[cfg(test)]
mod test {
    use super::{HazardRateFn, ScaledRateFn};
    use crate::rates::fixed_rate::FixedRateFn;

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_scaled_rate_fn() {
        let base_rate = FixedRateFn::new(0.5, 5.0).unwrap();
        let scaled_rate = ScaledRateFn::new(&base_rate, 2.0, 3.0);
        assert_eq!(scaled_rate.rate(5.0), 1.0);
        assert_eq!(scaled_rate.cum_rate(5.0), 5.0);
        assert_eq!(scaled_rate.inverse_cum_rate(2.0), Some(2.0));
        assert_eq!(scaled_rate.inverse_cum_rate(5.0), None);
    }
}
