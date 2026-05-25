mod generate;
mod model;
mod parse;
mod serialize;
pub mod serve;
mod templates;

pub use generate::{GroupEntry, assemble_layout};
pub use model::*;
#[cfg(test)]
pub use templates::load_group_template;
pub use templates::{TEMPLATES, TemplateEntry};

#[cfg(test)]
mod tests;
