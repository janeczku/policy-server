use anyhow::Result;

use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::collections::HashMap;

use std::fs::File;

#[derive(Deserialize, Debug, Clone)]
pub struct Policy {
    pub url: String,

    #[serde(skip)]
    pub wasm_module_path: String,

    #[serde(flatten)]
    extra_fields: HashMap<String, Value>,
}

impl Policy {
    pub fn settings(&self) -> Mapping {
        self.extra_fields
            .get("settings")
            .map(|v| {
                if v.is_null() || !v.is_mapping() {
                    None
                } else {
                    v.as_mapping()
                }
            })
            .flatten()
            .unwrap_or(&Mapping::new())
            .clone()
    }
}

// Reads the policies configuration file, returns a HashMap with String as value
// and Policy as values. The key is the name of the policy as provided by the user
// inside of the configuration file. This name is used to build the API path
// exposing the policy.
pub fn read_policies_file(path: &str) -> Result<HashMap<String, Policy>> {
    let settings_file = File::open(path)?;
    let ps: HashMap<String, Policy> = serde_yaml::from_reader(&settings_file)?;
    Ok(ps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_settings_when_data_is_provided() {
        let input = r#"
---
example:
  url: file:///tmp/namespace-validate-policy.wasm
  settings:
    valid_namespace: valid
"#;
        let policies: HashMap<String, Policy> = serde_yaml::from_str(&input).unwrap();
        assert_eq!(policies.is_empty(), false);

        let policy = policies.get("example").unwrap();
        let settings = policy.settings();
        assert_ne!(settings.is_empty(), true);
    }

    #[test]
    fn get_settings_when_empty_map_is_provided() {
        let input = r#"
---
example:
  url: file:///tmp/namespace-validate-policy.wasm
  settings: {}
"#;

        let policies: HashMap<String, Policy> = serde_yaml::from_str(&input).unwrap();
        assert_eq!(policies.is_empty(), false);

        let policy = policies.get("example").unwrap();
        let settings = policy.settings();
        assert!(settings.is_empty());
    }

    #[test]
    fn get_settings_when_no_settings_are_provided() {
        let input = r#"
---
example:
  url: file:///tmp/namespace-validate-policy.wasm
"#;

        let policies: HashMap<String, Policy> = serde_yaml::from_str(&input).unwrap();
        assert_eq!(policies.is_empty(), false);

        let policy = policies.get("example").unwrap();
        let settings = policy.settings();
        assert!(settings.is_empty());
    }

    #[test]
    fn get_settings_when_settings_is_null() {
        let input = r#"
{
    "privileged-pods": {
        "url": "registry://ghcr.io/kubewarden/policies/pod-privileged:v0.1.5",
        "settings": null
    }
}
"#;

        let policies: HashMap<String, Policy> = serde_yaml::from_str(&input).unwrap();
        assert_eq!(policies.is_empty(), false);

        let policy = policies.get("privileged-pods").unwrap();
        let settings = policy.settings();
        assert!(settings.is_empty());
    }
}
