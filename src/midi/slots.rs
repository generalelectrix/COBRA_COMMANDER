//! Pure logic for MIDI slot naming and count calculation.

pub const CLOCK_WING_SLOT: &str = "Clock Wing";
const SUBMASTER_WING_PREFIX: &str = "Submaster Wing ";

pub fn submaster_wing_name(one_indexed: usize) -> String {
    format!("{SUBMASTER_WING_PREFIX}{one_indexed}")
}

/// Return true if the given slot name is a submaster wing slot.
pub fn is_submaster_wing(name: &str) -> bool {
    name.starts_with(SUBMASTER_WING_PREFIX)
}

/// At least 1, then one per 8 channels.
pub fn submaster_wing_count(channel_count: usize) -> usize {
    1.max(channel_count.div_ceil(8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wing_name() {
        assert_eq!(submaster_wing_name(1), "Submaster Wing 1");
        assert_eq!(submaster_wing_name(3), "Submaster Wing 3");
    }

    #[test]
    fn wing_count_zero_channels() {
        assert_eq!(submaster_wing_count(0), 1);
    }

    #[test]
    fn wing_count_one_channel() {
        assert_eq!(submaster_wing_count(1), 1);
    }

    #[test]
    fn wing_count_seven_channels() {
        assert_eq!(submaster_wing_count(7), 1);
    }

    #[test]
    fn wing_count_eight_channels() {
        assert_eq!(submaster_wing_count(8), 1);
    }

    #[test]
    fn wing_count_nine_channels() {
        assert_eq!(submaster_wing_count(9), 2);
    }

    #[test]
    fn wing_count_sixteen_channels() {
        assert_eq!(submaster_wing_count(16), 2);
    }

    #[test]
    fn wing_count_seventeen_channels() {
        assert_eq!(submaster_wing_count(17), 3);
    }
}
