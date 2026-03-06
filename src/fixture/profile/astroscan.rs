//! Clay Paky Astroscan - drunken sailor extraordinaire
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 9]
#[strobe(Long)]
pub struct Astroscan {
    lamp_on: BoolChannel,
    #[channel_control]
    #[animate]
    shutter: ChannelLevelUnipolar<UnipolarChannel>,
    #[animate]
    iris: UnipolarChannel,
    color: LabeledSelect,
    gobo: IndexedSelectMult,
    #[channel_control]
    #[animate]
    mirror_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[channel_control]
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    pan: BipolarChannelMirror,
    #[animate]
    tilt: BipolarChannelMirror,
}

impl Default for Astroscan {
    fn default() -> Self {
        Self {
            lamp_on: Bool::full_channel("LampOn", 2),
            shutter: Unipolar::channel("Dimmer", 3, 0, 139)
                .strobed()
                .with_channel_level(),
            iris: Unipolar::full_channel("Iris", 0),
            color: LabeledSelect::new(
                "Color",
                1,
                vec![
                    ("Open", 0),
                    ("Red", 14),
                    ("Yellow", 32),
                    ("Violet", 51),
                    ("Green", 67),
                    ("Orange", 81),
                    ("Blue", 98),
                    ("Pink", 115), // 127 back to white
                ],
            ),
            gobo: IndexedSelect::multiple("Gobo", 6, false, 5, 55, 0),
            gobo_rotation: Bipolar::split_channel("GoboRotation", 7, 189, 128, 193, 255, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            mirror_rotation: Bipolar::split_channel("MirrorRotation", 8, 189, 128, 193, 255, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            pan: Bipolar::channel("Pan", 4, 0, 255)
                .with_detent()
                .with_mirroring(true),
            tilt: Bipolar::channel("Tilt", 5, 0, 255)
                .with_detent()
                .with_mirroring(false),
        }
    }
}

impl AnimatedFixture for Astroscan {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.iris.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Iris),
            dmx_buf,
        );
        self.color
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.lamp_on
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.shutter.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Shutter),
            dmx_buf,
        );
        self.pan.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        );
        self.gobo
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.gobo_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        );
        self.mirror_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::MirrorRotation),
            dmx_buf,
        );
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixture::control::{DescribeOscControls, OscControlType};

    #[test]
    fn test_describe_controls() {
        let fixture = Astroscan::default();
        let controls = fixture.describe_controls();
        let names: Vec<&str> = controls.iter().map(|c| c.name.as_str()).collect();

        assert!(names.contains(&"LampOn"));
        assert!(names.contains(&"Dimmer"));
        assert!(names.contains(&"Iris"));
        assert!(names.contains(&"Color"));
        assert!(names.contains(&"Gobo"));
        assert!(names.contains(&"MirrorRotation"));
        assert!(names.contains(&"MirrorMirrorRotation"));
        assert!(names.contains(&"GoboRotation"));
        assert!(names.contains(&"MirrorGoboRotation"));
        assert!(names.contains(&"Pan"));
        assert!(names.contains(&"MirrorPan"));
        assert!(names.contains(&"Tilt"));
        assert!(names.contains(&"MirrorTilt"));

        // Verify control types
        let lamp_on = controls.iter().find(|c| c.name == "LampOn").unwrap();
        assert_eq!(lamp_on.control_type, OscControlType::Bool);

        let dimmer = controls.iter().find(|c| c.name == "Dimmer").unwrap();
        assert_eq!(dimmer.control_type, OscControlType::Unipolar);

        let color = controls.iter().find(|c| c.name == "Color").unwrap();
        assert!(matches!(
            &color.control_type,
            OscControlType::LabeledSelect { labels } if labels.len() == 8
        ));

        let gobo = controls.iter().find(|c| c.name == "Gobo").unwrap();
        assert_eq!(
            gobo.control_type,
            OscControlType::IndexedSelect {
                n: 5,
                x_primary_coordinate: false,
            }
        );

        let pan = controls.iter().find(|c| c.name == "Pan").unwrap();
        assert_eq!(pan.control_type, OscControlType::Bipolar);

        let mirror_pan = controls.iter().find(|c| c.name == "MirrorPan").unwrap();
        assert_eq!(mirror_pan.control_type, OscControlType::Bool);
    }
}
