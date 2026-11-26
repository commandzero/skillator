//! Configuration paths and captured location-expansion environment.

use super::*;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub(super) home: PathBuf,
    pub(super) environment: BTreeMap<String, String>,
}

impl AppPaths {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            // Location expressions are UTF-8; unrepresentable variables are unavailable.
            // Keep OS paths in their native form for the workflow's own validation.
            environment: std::env::vars_os()
                .filter_map(|(key, value)| {
                    Some((key.into_string().ok()?, value.into_string().ok()?))
                })
                .collect(),
        }
    }

    pub fn with_environment(home: PathBuf, environment: BTreeMap<String, String>) -> Self {
        Self { home, environment }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn library_config(&self) -> PathBuf {
        self.home.join(".skillator/library.yaml")
    }

    pub fn user_config(&self) -> PathBuf {
        self.home.join(".agents/skillator.yaml")
    }

    pub fn target_registry(&self) -> PathBuf {
        self.home.join(".skillator/targets.yaml")
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}
