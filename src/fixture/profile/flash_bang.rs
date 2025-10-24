//! Profile for the Monoprice "Flash Bang" 5-ring LED strobe.
//!
//! The profile is designed to use the direct-control 5-channel mode where
//! the brightness of each LED ring is directly controlled. There is also a
//! special 10-channel mode provided for using a pair of these fixtures where
//! the patterns can be extended over both arrays for additional effects.
use crate::fixture::prelude::*;

#[derive(EmitState, Control)]
#[strobe]
pub struct FlashBang {
    #[channel_control]
    #[animate]
    intensity: ChannelKnobUnipolar<Unipolar<()>>,
}

impl Default for FlashBang {
    fn default() -> Self {
        Self {
            intensity: Unipolar::new("Intensity", ())
                .at(UnipolarFloat::new(0.1))
                .with_channel_knob(0),
        }
    }
}
