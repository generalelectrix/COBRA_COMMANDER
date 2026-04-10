use eframe::egui;

use crate::config::Options;
use crate::fixture::patch::PatchOption;
use crate::ui_util::STATUS_COLORS;

use super::address_map::{AddressMap, GroupName};
use super::working_copy::PatchWorkingCopy;

pub fn default_for_option(opt: &PatchOption) -> String {
    match opt {
        PatchOption::Bool => "false".to_string(),
        PatchOption::Int => "0".to_string(),
        PatchOption::Select(choices) => choices.first().cloned().unwrap_or_default(),
        PatchOption::Url => String::new(),
        PatchOption::SocketAddr => String::new(),
        PatchOption::Optional(_) => String::new(),
    }
}

pub fn validate_option(opt: &PatchOption, value: &str) -> Result<(), String> {
    match opt {
        PatchOption::Bool | PatchOption::Select(_) => Ok(()),
        PatchOption::Int => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "must be a number".to_string()),
        PatchOption::Url => url::Url::parse(value)
            .map(|_| ())
            .map_err(|e| format!("invalid URL: {e}")),
        PatchOption::SocketAddr => value
            .parse::<std::net::SocketAddr>()
            .map(|_| ())
            .map_err(|_| "invalid address (expected host:port)".to_string()),
        PatchOption::Optional(inner) => {
            if value.is_empty() {
                Ok(())
            } else {
                validate_option(inner, value)
            }
        }
    }
}

pub fn render_option_widget(ui: &mut egui::Ui, key: &str, opt: &PatchOption, value: &mut String) {
    let key_with_colon = format!("{key}:");
    match opt {
        PatchOption::Bool => {
            let mut checked = value == "true";
            if ui.checkbox(&mut checked, key).changed() {
                *value = checked.to_string();
            }
        }
        PatchOption::Select(choices) => {
            ui.horizontal(|ui| {
                ui.label(&key_with_colon);
                egui::ComboBox::from_id_salt(key)
                    .selected_text(value.as_str())
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            ui.selectable_value(value, choice.clone(), choice);
                        }
                    });
            });
        }
        PatchOption::Int | PatchOption::Url | PatchOption::SocketAddr => {
            ui.horizontal(|ui| {
                ui.label(&key_with_colon);
                ui.text_edit_singleline(value);
            });
        }
        PatchOption::Optional(inner) => match inner.as_ref() {
            PatchOption::Select(choices) => {
                ui.horizontal(|ui| {
                    ui.label(&key_with_colon);
                    egui::ComboBox::from_id_salt(key)
                        .selected_text(if value.is_empty() {
                            "(none)"
                        } else {
                            value.as_str()
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(value, String::new(), "(none)");
                            for choice in choices {
                                ui.selectable_value(value, choice.clone(), choice);
                            }
                        });
                });
            }
            other => render_option_widget(ui, key, other, value),
        },
    }
}

pub fn build_options_from_form(entries: &[(String, String)]) -> Options {
    Options::from_entries(entries.iter().map(|(k, v)| {
        let yaml_val = if v.is_empty() {
            serde_yaml::Value::Null
        } else if v == "true" {
            serde_yaml::Value::Bool(true)
        } else if v == "false" {
            serde_yaml::Value::Bool(false)
        } else if let Ok(n) = v.parse::<i64>() {
            serde_yaml::Value::Number(n.into())
        } else {
            serde_yaml::Value::String(v.clone())
        };
        (k.clone(), yaml_val)
    }))
}

pub fn render_address_map(ui: &mut egui::Ui, wc: &PatchWorkingCopy, addr_map: &AddressMap) {
    if wc.groups.is_empty() || addr_map.is_empty() {
        ui.label("No addresses in use.");
        return;
    }

    for universe in addr_map.universes() {
        ui.label(format!("Universe {universe}"));

        let entries: Vec<_> = addr_map.range_for_universe(universe).collect();

        if entries.is_empty() {
            ui.label("  (empty)");
            continue;
        }

        // Group into contiguous runs of the same group.
        let mut ranges: Vec<(usize, usize, GroupName, bool)> = Vec::new();

        for (ua, names) in &entries {
            let addr = ua.address;
            let name = &names[0];
            let is_collision = names.len() > 1;

            if let Some(last) = ranges.last_mut()
                && last.2 == *name
                && last.1 + 1 == addr
                && last.3 == is_collision
            {
                last.1 = addr;
                continue;
            }
            ranges.push((addr, addr, name.clone(), is_collision));
        }

        // Render ranges with gaps shown between them.
        let mut prev_end: usize = 0;
        for (start, end, name, is_collision) in &ranges {
            if *start > prev_end + 1 {
                let gap_start = prev_end + 1;
                let gap_end = start - 1;
                let gap_str = if gap_start == gap_end {
                    format!("{gap_start:>3}")
                } else {
                    format!("{gap_start:>3}-{gap_end}")
                };
                ui.colored_label(
                    ui.style().visuals.text_color().gamma_multiply(0.3),
                    format!("{gap_str}  (free)"),
                );
            }

            let range_str = if start == end {
                format!("{start:>3}")
            } else {
                format!("{start:>3}-{end}")
            };

            let label = format!("{range_str}  {name}");
            if *is_collision {
                ui.colored_label(STATUS_COLORS.error, &label)
                    .on_hover_text("DMX address collision!");
            } else {
                ui.label(&label);
            }
            prev_end = *end;
        }

        // Show trailing free space.
        if prev_end < 512 {
            let gap_str = if prev_end + 1 == 512 {
                "512".to_string()
            } else {
                format!("{}-512", prev_end + 1)
            };
            ui.colored_label(
                ui.style().visuals.text_color().gamma_multiply(0.3),
                format!("{gap_str}  (free)"),
            );
        }

        ui.add_space(4.0);
    }
}
