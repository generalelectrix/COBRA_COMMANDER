mod generate;
mod model;
mod parse;
mod regroup;
mod serialize;
mod templates;

pub use generate::{GroupEntry, generate_layout};
#[expect(unused)]
pub use model::*;
pub use parse::parse_touchosc;
#[expect(unused)]
pub use parse::parse_touchosc_bytes;
pub use regroup::set_group_name;
#[expect(unused)]
pub use serialize::write_touchosc;
#[expect(unused)]
pub use templates::load_base_template;
pub use templates::load_group_template;

#[cfg(test)]
mod tests;
