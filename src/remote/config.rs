use super::{Error, Result, state};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    version: u64,
    hosts: BTreeMap<String, Host>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Host {
    pub destination: String,
}

impl Config {
    pub fn load(home: &Path) -> Result<Self> {
        let path = home.join(".skillator/config.yaml");
        let bytes = state::read_contained(home, ".skillator/config.yaml")?.ok_or_else(|| {
            Error::input(format!(
                "cannot read {}: file is missing; configure SSH hosts before running library rsync",
                path.display()
            ))
        })?;
        let text = String::from_utf8(bytes).map_err(Error::input_display)?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self> {
        let config: Self = crate::config::parse_yaml(text).map_err(|issues| {
            Error::input(
                issues
                    .into_iter()
                    .map(|issue| issue.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        if config.version != 1 {
            return Err(Error::input(format!(
                "unsupported main configuration version {}; expected 1",
                config.version
            )));
        }
        for (alias, host) in &config.hosts {
            if alias == "local" || !identifier(alias) {
                return Err(Error::input(format!(
                    "invalid or reserved host alias {alias:?}"
                )));
            }
            // Destinations are SSH aliases or user@host, never options or shell expressions.
            let destination = &host.destination;
            if destination.is_empty()
                || destination.starts_with('-')
                || destination
                    .chars()
                    .any(|c| !c.is_ascii_alphanumeric() && !"@._-:[]".contains(c))
                || destination.matches('@').count() > 1
                || destination.starts_with('@')
                || destination.ends_with('@')
            {
                return Err(Error::input(format!(
                    "invalid SSH destination for host {alias}"
                )));
            }
        }
        Ok(config)
    }

    pub fn select(&self, requested: Option<&str>) -> Result<Vec<(String, Host)>> {
        if self.hosts.is_empty() {
            return Err(Error::input(
                "no SSH hosts configured in ~/.skillator/config.yaml",
            ));
        }
        let aliases: BTreeSet<&str> = match requested {
            None => self.hosts.keys().map(String::as_str).collect(),
            Some(requested) => {
                let mut aliases = BTreeSet::new();
                for alias in requested.split(',') {
                    if !self.hosts.contains_key(alias) {
                        return Err(Error::argument(format!(
                            "unknown or empty host alias {alias:?}"
                        )));
                    }
                    aliases.insert(alias);
                }
                aliases
            }
        };
        Ok(aliases
            .into_iter()
            .map(|alias| (alias.to_owned(), self.hosts[alias].clone()))
            .collect())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_configuration_link_outside_home_is_rejected() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".skillator")).unwrap();
        std::fs::write(
            outside.path().join("config.yaml"),
            "version: 1\nhosts: {remote: {destination: remote}}\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("config.yaml"),
            home.path().join(".skillator/config.yaml"),
        )
        .unwrap();
        assert!(Config::load(home.path()).is_err());
    }

    #[test]
    fn selection_is_strict_and_deterministic() {
        let config = Config::parse(
            "version: 1\nhosts:\n  b: {destination: user@b}\n  a: {destination: a}\n",
        )
        .unwrap();
        assert_eq!(
            config
                .select(None)
                .unwrap()
                .iter()
                .map(|(alias, _)| alias.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(config.select(Some("b,b")).unwrap().len(), 1);
        for selection in ["", "b,", "missing", "a, b"] {
            assert_eq!(config.select(Some(selection)).unwrap_err().code, 2);
        }
    }

    #[test]
    fn malformed_configuration_is_rejected() {
        for text in [
            "version: 2\nhosts: {}",
            "version: 1\nhosts: {}\nextra: true",
            "version: 1\nhosts:\n a: {destination: a}\n a: {destination: b}",
            "version: 1\nhosts: {local: {destination: a}}",
            "version: 1\nhosts: {a: {destination: '-oProxyCommand=evil'}}",
            "version: 1\nhosts: {a: {destination: 'a; touch file'}}",
            "version: 1\nhosts: {a: {destination: ''}}",
            "version: 1\nhosts: {a: {destination: a, extra: true}}",
            "version: 1\nhosts: {}\n---\nversion: 1\nhosts: {}",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text}");
        }
        assert!(
            Config::parse("version: 1\nhosts: {}")
                .unwrap()
                .select(None)
                .is_err()
        );
    }
}
