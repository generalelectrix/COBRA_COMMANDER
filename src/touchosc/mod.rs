mod generate;
mod model;
mod parse;
mod serialize;
pub mod serve;
mod templates;

#[expect(unused)]
pub use generate::generate_layout;
pub use generate::{GroupEntry, assemble_layout};
pub use model::*;
#[expect(unused)]
pub use parse::parse_touchosc;
#[expect(unused)]
pub use templates::BASE_TEMPLATE;
#[expect(unused)]
pub use templates::load_base_template;
#[expect(unused)]
pub use templates::load_group_template;
pub use templates::{TEMPLATES, TemplateEntry};

#[cfg(test)]
mod tests;
