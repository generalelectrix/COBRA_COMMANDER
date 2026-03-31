mod generate;
mod model;
mod parse;
mod serialize;
pub mod serve;
mod templates;

pub use generate::{GroupEntry, generate_layout};
pub use model::*;
pub use parse::parse_touchosc;
#[expect(unused)]
pub use templates::BASE_TEMPLATE;
#[expect(unused)]
pub use templates::load_base_template;
pub use templates::{TEMPLATES, TemplateEntry, load_group_template};

#[cfg(test)]
mod tests;
