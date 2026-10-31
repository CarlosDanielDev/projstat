use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::model::RepoRef;

/// How repositories are ordered in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Momentum,
    Progress,
    Name,
    Last,
}

impl FromStr for Sort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Sort> {
        match s.to_ascii_lowercase().as_str() {
            "momentum" => Ok(Sort::Momentum),
            "progress" => Ok(Sort::Progress),
            "name" => Ok(Sort::Name),
            "last" => Ok(Sort::Last),
            other => Err(anyhow!("unknown sort order: {other}")),
        }
    }
}

/// Parsed user configuration. `Default` is the no-config behaviour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub pinned: Vec<RepoRef>,
    pub sort: Sort,
    pub discover_orgs: Vec<String>,
}

/// On-disk shape; every field is optional so an empty file is valid.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    pinned: Vec<String>,
    sort: Option<String>,
    discover_orgs: Vec<String>,
}

/// Parses config TOML. Empty input yields `Config::default()`.
pub fn parse_config(toml_str: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(toml_str)?;
    let pinned = raw
        .pinned
        .iter()
        .map(|s| RepoRef::parse(s).ok_or_else(|| anyhow!("invalid pinned repo: {s}")))
        .collect::<Result<Vec<_>>>()?;
    let sort = match raw.sort {
        Some(s) => s.parse()?,
        None => Sort::default(),
    };
    Ok(Config {
        pinned,
        sort,
        discover_orgs: raw.discover_orgs,
    })
}

/// Loads the config from `config_path()`. Returns the default when the file
/// is missing or fails to parse; never panics.
pub fn load() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| parse_config(&s).ok())
        .unwrap_or_default()
}

/// `$XDG_CONFIG_HOME/projstat/config.toml`, falling back to
/// `$HOME/.config/projstat/config.toml`.
pub fn config_path() -> PathBuf {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn config_path_from(xdg_config_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    let base = match xdg_config_home {
        Some(xdg) => PathBuf::from(xdg),
        None => PathBuf::from(home.unwrap_or_default()).join(".config"),
    };
    base.join("projstat").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
pinned = ["a/b"]
sort = "progress"
discover_orgs = ["hawk-oliver"]
"#;

    #[test]
    fn full_sample_parses_to_expected_struct() {
        let cfg = parse_config(SAMPLE).expect("sample is valid");
        assert_eq!(
            cfg,
            Config {
                pinned: vec![RepoRef::parse("a/b").unwrap()],
                sort: Sort::Progress,
                discover_orgs: vec!["hawk-oliver".to_string()],
            }
        );
    }

    #[test]
    fn empty_string_yields_default() {
        let cfg = parse_config("").expect("empty is valid");
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.sort, Sort::Momentum);
        assert!(cfg.pinned.is_empty());
        assert!(cfg.discover_orgs.is_empty());
    }

    #[test]
    fn invalid_pin_is_an_error() {
        assert!(parse_config(r#"pinned = ["not-a-repo"]"#).is_err());
    }

    #[test]
    fn unknown_sort_is_an_error() {
        assert!(parse_config(r#"sort = "bogus""#).is_err());
    }

    #[test]
    fn sort_parses_case_insensitively() {
        assert_eq!("Name".parse::<Sort>().unwrap(), Sort::Name);
        assert_eq!("LAST".parse::<Sort>().unwrap(), Sort::Last);
    }

    #[test]
    fn config_path_prefers_xdg_then_home() {
        assert_eq!(
            config_path_from(Some("/x".into()), Some("/h".into())),
            PathBuf::from("/x/projstat/config.toml")
        );
        assert_eq!(
            config_path_from(None, Some("/h".into())),
            PathBuf::from("/h/.config/projstat/config.toml")
        );
    }
}
