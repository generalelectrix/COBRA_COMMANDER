pub mod aquarius;
pub mod astera;
pub mod astroscan;
pub mod color;
pub mod colordynamic;
pub mod comet;
pub mod cosmic_burst;
pub mod dimmer;
pub mod empty_channel;
pub mod faderboard;
pub mod flash_bang;
pub mod freedom_fries;
pub mod freq_strobe;
pub mod fusion_roll;
pub mod h2o;
pub mod hypnotic;
pub mod iwash_led;
pub mod leko;
pub mod lumasphere;
pub mod lumitone;
pub mod quadphase;
pub mod radiance;
pub mod rotosphere_q3;
pub mod rush_wizard;
pub mod solar_system;
pub mod starlight;
pub mod swarmolon;
pub mod triphase;
pub mod ufo;
pub mod uv_led_brick;
pub mod venus;
pub mod wizard_extreme;
pub mod wizlet;
pub mod wled;

#[cfg(test)]
mod osc_control_test {
    use rosc::{OscMessage, OscType};

    use crate::channel::mock::no_op_emitter;
    use crate::config::{FixtureGroupKey, Options};
    use crate::fixture::control::{OscControlDescription, OscControlType};
    use crate::fixture::patch::PATCHERS;
    use crate::osc::{OscClientId, OscControlMessage};

    /// Fixtures that cannot be constructed from their declared options menu alone.
    /// These require complex option types not representable by PatchOption.
    const EXCLUDED_FIXTURES: &[&str] = &["RugDoctor"];

    /// Generate fuzz (addr, arg) pairs for a control based on its type.
    fn fuzz_values(
        key: &FixtureGroupKey,
        control: &OscControlDescription,
    ) -> Vec<(String, OscType)> {
        let base = format!("/{}/{}", key.0, control.name);
        match &control.control_type {
            OscControlType::Unipolar | OscControlType::Phase => vec![
                (base.clone(), OscType::Float(0.0)),
                (base.clone(), OscType::Float(0.5)),
                (base.clone(), OscType::Float(1.0)),
                (base.clone(), OscType::Float(-0.1)),
                (base.clone(), OscType::Float(1.1)),
                (base.clone(), OscType::Float(f32::MIN_POSITIVE)),
            ],
            OscControlType::Bipolar => vec![
                (base.clone(), OscType::Float(-1.0)),
                (base.clone(), OscType::Float(0.0)),
                (base.clone(), OscType::Float(1.0)),
                (base.clone(), OscType::Float(-1.1)),
                (base.clone(), OscType::Float(1.1)),
            ],
            OscControlType::Bool => vec![
                (base.clone(), OscType::Bool(true)),
                (base.clone(), OscType::Bool(false)),
                (base.clone(), OscType::Int(0)),
                (base.clone(), OscType::Int(1)),
                (base.clone(), OscType::Float(0.0)),
                (base.clone(), OscType::Float(1.0)),
            ],
            OscControlType::LabeledSelect { labels } => labels
                .iter()
                .map(|l| (format!("{}/{}", base, l), OscType::Float(1.0)))
                .collect(),
            OscControlType::IndexedSelect {
                n,
                x_primary_coordinate,
            } => (0..*n)
                .map(|i| {
                    let (x, y) = if *x_primary_coordinate {
                        (i + 1, 1)
                    } else {
                        (1, i + 1)
                    };
                    (format!("{}/{}/{}", base, x, y), OscType::Float(1.0))
                })
                .collect(),
        }
    }

    #[test]
    fn test_all_fixtures_handle_declared_controls() {
        let client_id = OscClientId::example();

        for patcher in PATCHERS.iter() {
            if EXCLUDED_FIXTURES.contains(&patcher.name.0) {
                continue;
            }

            let key = FixtureGroupKey(format!("test_{}", patcher.name));

            let mut group = match (patcher.create_group)(key.clone(), Default::default()) {
                Ok(group) => group,
                Err(_) => {
                    // Try again with example values from the options menu.
                    let menu = (patcher.group_options)();
                    assert!(
                        !menu.is_empty(),
                        "{}: create_group failed with default options but declares no options",
                        patcher.name
                    );
                    let options = Options::from_entries(
                        menu.iter()
                            .map(|(name, opt)| (name.clone(), opt.example_value())),
                    );
                    (patcher.create_group)(key.clone(), options).unwrap_or_else(|e| {
                        panic!(
                            "{}: create_group failed even with example options: {e}",
                            patcher.name
                        )
                    })
                }
            };

            let controls = group.describe_controls();

            for control in &controls {
                for (addr, arg) in fuzz_values(&key, control) {
                    let msg = OscControlMessage::new(
                        OscMessage {
                            addr: addr.clone(),
                            args: vec![arg.clone()],
                        },
                        client_id,
                    )
                    .expect("valid OSC message");

                    let result = group.control(&msg, no_op_emitter());
                    assert!(
                        result.is_ok(),
                        "{} control {:?} failed with addr={}, arg={:?}: {}",
                        patcher.name,
                        control.name,
                        addr,
                        arg,
                        result.unwrap_err()
                    );
                }
            }
        }
    }
}
