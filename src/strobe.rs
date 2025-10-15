//! A strobing system based on a special-purpose clock/animator pair.
//!
//! This system should work for any fixture that can effectively strobe by
//! modulating the level control with a tuned square wave.  Due to the low DMX
//! framerate of, say, 40-50 Hz, we can't really achieve strobing any better than
//! 10 flashes per second or so. The fact that the physical DMX output is
//! usually unsynchronized with frame writing implies frame tearing, which will
//! also impact the quality of strobing achieved with this system when attempting
//! to hit relatively high strobe rates.
//!
//! The advantage vs. using any given onboard strobe control is that we can
//! easily synchronize the strobing of multiple fixture types across the rig.
