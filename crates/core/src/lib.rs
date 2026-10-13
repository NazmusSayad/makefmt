mod format;
mod justfile;
pub mod shfmt;

use std::num::NonZeroUsize;

use serde::Deserialize;

pub use format::format;
pub use justfile::format_justfile;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub align_across_comments: bool,
    pub align_assignments: bool,
    pub align_continuations: bool,
    pub format_recipes: bool,
    pub indent_conditionals: bool,
    pub indent_width: usize,
    pub max_blank_lines: NonZeroUsize,
    pub trim_trailing_whitespace: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            align_across_comments: false,
            align_assignments: true,
            align_continuations: true,
            format_recipes: true,
            indent_conditionals: true,
            indent_width: 2,
            max_blank_lines: NonZeroUsize::MIN,
            trim_trailing_whitespace: false,
        }
    }
}

impl Options {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }
}
