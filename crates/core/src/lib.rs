mod format;

use serde::Deserialize;

pub use format::format;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub align_assignments: bool,
    pub align_continuations: bool,
    pub indent_conditionals: bool,
    pub indent_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            align_assignments: true,
            align_continuations: true,
            indent_conditionals: true,
            indent_width: 2,
        }
    }
}

impl Options {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }
}
