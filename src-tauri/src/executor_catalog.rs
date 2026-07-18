//! Production execution settings catalog.
//!
//! The daemon/native capability registry owns actual tool execution.  This
//! module only provides stable defaults for persisted UI/runtime settings so
//! settings code does not depend on the retired executor run path.

use std::collections::HashMap;

const DEFAULT_ENABLED_TOOLS: &[(&str, bool)] = &[
    ("read_file", true),
    ("list_dir", true),
    ("write_file", true),
    ("write_module", true),
    ("run_terminal", false),
    ("lint_module", true),
];

pub fn default_enabled_tools() -> HashMap<String, bool> {
    DEFAULT_ENABLED_TOOLS
        .iter()
        .map(|(name, enabled)| ((*name).to_string(), *enabled))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::default_enabled_tools;

    #[test]
    fn default_tools_match_expected_production_defaults() {
        let defaults = default_enabled_tools();

        assert_eq!(defaults.get("read_file"), Some(&true));
        assert_eq!(defaults.get("list_dir"), Some(&true));
        assert_eq!(defaults.get("write_file"), Some(&true));
        assert_eq!(defaults.get("write_module"), Some(&true));
        assert_eq!(defaults.get("run_terminal"), Some(&false));
        assert_eq!(defaults.get("lint_module"), Some(&true));
    }
}
