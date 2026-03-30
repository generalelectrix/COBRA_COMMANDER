mod model;
mod parse;
mod serialize;
mod templates;

pub use model::*;
pub use parse::parse_touchosc;
#[expect(unused)]
pub use parse::parse_touchosc_bytes;
#[expect(unused)]
pub use serialize::write_touchosc;
#[expect(unused)]
pub use templates::load_base_template;
pub use templates::load_group_template;

#[cfg(test)]
mod tests;
