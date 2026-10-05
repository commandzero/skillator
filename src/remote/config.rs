use super::{Error, Result, transport::valid_destination};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    version: u64,
    #[serde(default)]
    hosts: Option<BTreeMap<String, Host>>,
    #[serde(default)]
    leader: Option<Host>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Host {
    pub destination: String,
}

impl Config {
    pub fn load(home: &Path) -> Result<Self> {
        let directory = home.join(".skillator");
        let path = directory.join("config.yaml");
        let parent = fs::symlink_metadata(&directory)
            .map_err(|error| Error::input(format!("cannot read {}: {error}", path.display())))?;
        if parent.file_type().is_symlink() || !parent.is_dir() {
            return Err(Error::input(format!(
                "unsafe configuration directory {}",
                directory.display()
            )));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| Error::input(format!("cannot read {}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::input(format!(
                "unsafe configuration file {}",
                path.display()
            )));
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| Error::input(format!("cannot read {}: {error}", path.display())))?;
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
        match (&config.hosts, &config.leader) {
            (Some(hosts), None) if !hosts.is_empty() => {
                for (alias, host) in hosts {
                    if alias == "local" || !identifier(alias) {
                        return Err(Error::input(format!(
                            "invalid or reserved host alias {alias:?}"
                        )));
                    }
                    validate_destination(&host.destination, alias)?;
                }
            }
            (None, Some(leader)) => validate_destination(&leader.destination, "leader")?,
            _ => {
                return Err(Error::input(
                    "configure either a nonempty hosts map or one leader, not both",
                ));
            }
        }
        Ok(config)
    }

    pub fn leader(&self) -> Option<&Host> {
        self.leader.as_ref()
    }

    pub fn select(&self, requested: Option<&str>) -> Result<Vec<(&str, &Host)>> {
        let Some(hosts) = self.hosts.as_ref() else {
            return Err(Error::argument("--hosts is only available on a leader"));
        };
        let Some(requested) = requested else {
            return Ok(hosts
                .iter()
                .map(|(alias, host)| (alias.as_str(), host))
                .collect());
        };
        let mut aliases = BTreeSet::new();
        for alias in requested.split(',') {
            if !hosts.contains_key(alias) {
                return Err(Error::argument(format!(
                    "unknown or empty host alias {alias:?}"
                )));
            }
            aliases.insert(alias);
        }
        Ok(aliases
            .into_iter()
            .map(|alias| {
                let (alias, host) = hosts.get_key_value(alias).expect("selected host exists");
                (alias.as_str(), host)
            })
            .collect())
    }
}

fn validate_destination(destination: &str, alias: &str) -> Result<()> {
    if !valid_destination(destination) {
        return Err(Error::input(format!(
            "invalid SSH destination for host {alias}"
        )));
    }
    Ok(())
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
    fn host_configuration_directory_link_is_rejected() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("config.yaml"),
            "version: 1\nhosts: {remote: {destination: remote}}\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join(".skillator")).unwrap();
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
                .map(|(alias, _)| *alias)
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(config.select(Some("b,b")).unwrap().len(), 1);
        for selection in ["", "b,", "missing", "a, b"] {
            assert_eq!(config.select(Some(selection)).unwrap_err().code, 2);
        }
        assert!(config.leader().is_none());
    }

    #[test]
    fn follower_has_one_leader_and_rejects_host_selection() {
        let config = Config::parse("version: 1\nleader: {destination: user@main}\n").unwrap();
        assert_eq!(config.leader().unwrap().destination, "user@main");
        assert_eq!(config.select(Some("dev")).unwrap_err().code, 2);
        assert_eq!(config.select(None).unwrap_err().code, 2);
    }

    #[test]
    fn destinations_reject_rsync_host_path_confusion_for_both_roles() {
        for destination in [
            "local:prod",
            "user@local:prod",
            "::1",
            "host::module",
            "host:22",
            "host:/path",
            "[::1]:22",
            "[not-ipv6]",
            "[::1",
            "::1]",
            "user@[::1",
            "user@[::1]extra",
            "@host",
            "user@",
            "user@@host",
            "-host",
            "-user@host",
            "user@-host",
        ] {
            for config in [
                format!("version: 1\nhosts:\n  peer:\n    destination: '{destination}'\n"),
                format!("version: 1\nleader:\n  destination: '{destination}'\n"),
            ] {
                assert!(
                    Config::parse(&config).is_err(),
                    "accepted {destination} in {config}"
                );
            }
        }
        for destination in [
            "prod",
            "ssh_alias-1",
            "user@host.example",
            "[::1]",
            "user@[2001:db8::1]",
        ] {
            for config in [
                format!("version: 1\nhosts:\n  peer:\n    destination: '{destination}'\n"),
                format!("version: 1\nleader:\n  destination: '{destination}'\n"),
            ] {
                assert!(
                    Config::parse(&config).is_ok(),
                    "rejected {destination} in {config}"
                );
            }
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
            "version: 1\nhosts: {}",
            "version: 1",
            "version: 1\nleader: {destination: ''}",
            "version: 1\nleader: {destination: '-oProxyCommand=evil'}",
            "version: 1\nleader: {destination: a}\nhosts: {b: {destination: b}}",
            "version: 1\nleader: {destination: a, extra: true}",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text}");
        }
    }
}
