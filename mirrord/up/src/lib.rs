//! Configuration loading and validation for the `mirrord up` command.

use std::path::PathBuf;

use miette::Diagnostic;
use mirrord_config::config::{ConfigContext, ConfigError, MirrordConfig};
use thiserror::Error;

mod config;

pub use config::UpConfig;
use config::UpFileConfig;

/// Errors produced by `mirrord up` command.
#[derive(Debug, Error, Diagnostic)]
pub enum UpError {
    #[error("failed to read mirrord-up config: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse mirrord-up config: {0}")]
    #[diagnostic(help("Check the YAML syntax and field names in your mirrord-up.yaml."))]
    Parse(#[from] serde_yaml::Error),

    #[error("mirrord-up config validation failed: {0}")]
    Validation(#[from] ConfigError),
}

/// Load and parse a `mirrord-up.yaml` configuration file.
pub fn load_up_config(path: &PathBuf) -> Result<UpConfig, UpError> {
    let content = std::fs::read_to_string(path)?;
    let file_config: UpFileConfig = serde_yaml::from_str(&content)?;
    let mut context = ConfigContext::default();
    let config = file_config.generate_config(&mut context)?;
    Ok(config)
}
