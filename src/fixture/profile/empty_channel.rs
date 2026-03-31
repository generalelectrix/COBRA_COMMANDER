//! A stupid hack to offset the channel selectors with an empty fixture.

use crate::fixture::prelude::*;

#[derive(Default, Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 0]
#[no_touchosc_template]
pub struct EmptyChannel {}

impl NonAnimatedFixture for EmptyChannel {
    fn render(&self, _group_controls: &FixtureGroupControls, _dmx_buffer: &mut [u8]) {}
}
