use std::{
    ops::{AddAssign, Sub},
    time::Duration,
};

use number::{BipolarFloat, UnipolarFloat};

#[allow(unused)]
pub fn quadratic(v: UnipolarFloat) -> UnipolarFloat {
    UnipolarFloat::new(v.val().powi(2))
}

/// Scale value into the provided integer range.
/// The range is inclusive at both ends.
#[inline(always)]
pub fn unipolar_to_range(start: u8, end: u8, value: UnipolarFloat) -> u8 {
    if end > start {
        ((end - start) as f64 * value.val()) as u8 + start
    } else {
        ((start - end) as f64 * value.invert().val()) as u8 + end
    }
}

/// Coerce the bottom 5% of the fader range to be a hard 0, and rescale the rest.
#[inline(always)]
pub fn unipolar_fader_with_detent(v: UnipolarFloat) -> UnipolarFloat {
    if v.val() < 0.05 {
        return UnipolarFloat::ZERO;
    }
    UnipolarFloat::new((v.val() - 0.05) / 0.95)
}

/// Coerce the center 5% of the fader range to be a hard 0, and rescale the rest.
#[inline(always)]
pub fn bipolar_fader_with_detent(v: BipolarFloat) -> BipolarFloat {
    let v = v.val();
    if v < 0.0 {
        if v > -0.05 {
            BipolarFloat::ZERO
        } else {
            BipolarFloat::new((v + 0.05) / 0.95)
        }
    } else if v < 0.05 {
        BipolarFloat::ZERO
    } else {
        BipolarFloat::new((v - 0.05) / 0.95)
    }
}

/// A fixture parameter that ramps to its setpoint at a finite rate.
#[derive(Debug)]
pub struct RampingParameter<P> {
    pub target: P,
    current: P,
    /// Units / sec for the parameter to ramp.
    ramp_rate: P,
}

impl<P: Copy + Sub<Output = P> + Into<f64> + AddAssign<f64>> RampingParameter<P> {
    pub fn new(initial_value: P, ramp_rate: P) -> Self {
        Self {
            target: initial_value,
            current: initial_value,
            ramp_rate,
        }
    }

    pub fn update(&mut self, delta_t: Duration) {
        let (target, current) = (self.target, self.current);
        let delta: f64 = (target - current).into();
        let ramp_rate: f64 = self.ramp_rate.into();
        let step = (ramp_rate * delta_t.as_secs_f64()).copysign(delta);
        if step.abs() > delta.abs() {
            self.current = self.target;
        } else {
            self.current += step;
        }
    }

    pub fn current(&self) -> P {
        self.current
    }
}

#[cfg(test)]
mod test {
    use super::unipolar_to_range;
    use number::UnipolarFloat;
    #[test]
    fn test_unipolar_to_range() {
        assert_eq!(0, unipolar_to_range(0, 255, UnipolarFloat::ZERO));
        assert_eq!(255, unipolar_to_range(0, 255, UnipolarFloat::ONE));
        assert_eq!(50, unipolar_to_range(0, 100, UnipolarFloat::new(0.5)));
        assert_eq!(255, unipolar_to_range(255, 0, UnipolarFloat::ZERO));
        assert_eq!(0, unipolar_to_range(255, 0, UnipolarFloat::ONE));
        assert_eq!(50, unipolar_to_range(100, 0, UnipolarFloat::new(0.5)));
    }
}
