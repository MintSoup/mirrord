/// Configuration types for `mirrord-up.yaml`.
///
/// Uses the two-layer config pattern from mirrord-config: file config structs (with
/// `Option<T>` fields, for flexible deserialization) are resolved into runtime config structs
/// (with concrete types and defaults applied) via `MirrordConfig::generate_config`.
use std::collections::HashMap;

use mirrord_config::{
    config::{
        ConfigContext, ConfigError, FromMirrordConfig, MirrordConfig, Result,
        source::MirrordConfigSource,
    },
    feature::{env::EnvConfig, network::incoming::http_filter::HttpFilterConfig},
    target::TargetConfig,
};
use mirrord_config_derive::MirrordConfig;
use serde::{Deserialize, Serialize};

/// Incoming traffic mode for a service.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceMode {
    #[default]
    Mirror,
    Replace,
    Steal,
}

/// How to run a service locally.
///
/// Externally tagged serde enum, so YAML looks like:
/// ```yaml
/// run:
///   exec:
///     command: ["node", "server.js"]
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RunConfig {
    Container { command: Vec<String> },
    Exec { command: Vec<String> },
}

/// Default settings applied to all services.
#[derive(MirrordConfig, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[config(map_to = "DefaultsFileConfig")]
pub struct DefaultsConfig {
    #[config(default = false)]
    pub accept_invalid_certificates: bool,
    #[config(default = true)]
    pub operator: bool,
    #[config(default = true)]
    pub telemetry: bool,
}

/// Per-service configuration.
#[derive(MirrordConfig, Clone, Debug, PartialEq)]
#[config(map_to = "ServiceFileConfig")]
pub struct ServiceConfig {
    #[config(nested)]
    pub target: TargetConfig,
    #[config(nested)]
    pub env: EnvConfig,
    #[config(default)]
    pub mode: ServiceMode,
    #[config(nested)]
    pub http_filter: HttpFilterConfig,
    pub run: RunConfig,
}

/// Resolved top-level `mirrord-up.yaml` configuration.
#[derive(Clone, Debug)]
pub struct UpConfig {
    pub defaults: DefaultsConfig,
    pub services: HashMap<String, ServiceConfig>,
}

/// File-format top-level configuration. All keys except `defaults` are treated as service names.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpFileConfig {
    #[serde(default)]
    pub defaults: Option<DefaultsFileConfig>,
    #[serde(flatten)]
    pub services: HashMap<String, ServiceFileConfig>,
}

impl MirrordConfig for UpFileConfig {
    type Generated = UpConfig;

    fn generate_config(self, context: &mut ConfigContext) -> Result<Self::Generated> {
        let defaults = self.defaults.unwrap_or_default().generate_config(context)?;
        let services = self
            .services
            .into_iter()
            .map(|(name, svc)| Ok((name, svc.generate_config(context)?)))
            .collect::<Result<HashMap<_, _>>>()?;
        Ok(UpConfig { defaults, services })
    }
}

impl FromMirrordConfig for UpConfig {
    type Generator = UpFileConfig;
}

#[cfg(test)]
mod tests {
    use mirrord_config::config::{ConfigContext, MirrordConfig};

    use super::*;

    /// Helper: parse YAML into UpConfig via the two-layer config system.
    fn parse(yaml: &str) -> UpConfig {
        let file_config: UpFileConfig = serde_yaml::from_str(yaml).unwrap();
        let mut context = ConfigContext::default();
        file_config.generate_config(&mut context).unwrap()
    }

    #[test]
    fn defaults_applied_when_omitted() {
        let config = parse(
            r#"
            my-svc:
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        assert_eq!(
            config.defaults,
            DefaultsConfig {
                accept_invalid_certificates: false,
                operator: true,
                telemetry: true,
            }
        );
    }

    #[test]
    fn defaults_overridden() {
        let config = parse(
            r#"
            defaults:
              accept_invalid_certificates: true
              operator: false
              telemetry: false
            my-svc:
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        assert_eq!(
            config.defaults,
            DefaultsConfig {
                accept_invalid_certificates: true,
                operator: false,
                telemetry: false,
            }
        );
    }

    #[test]
    fn service_with_all_fields() {
        let config = parse(
            r#"
            web:
              target:
                path: "deployment/web-app"
                namespace: "staging"
              env:
                override:
                  NODE_ENV: "development"
              mode: steal
              http_filter:
                header_filter: "x-session: local"
              run:
                container:
                  command: ["docker", "run", "-p", "8080:8080", "web:latest"]
            "#,
        );
        let svc = &config.services["web"];

        assert_eq!(
            svc.target.path.as_ref().unwrap().to_string(),
            "deployment/web-app"
        );
        assert_eq!(svc.target.namespace.as_deref(), Some("staging"));
        assert_eq!(
            svc.env.r#override.as_ref().unwrap()["NODE_ENV"],
            "development"
        );
        assert_eq!(svc.mode, ServiceMode::Steal);
        assert_eq!(
            svc.http_filter.header_filter.as_deref(),
            Some("x-session: local")
        );
        assert_eq!(
            svc.run,
            RunConfig::Container {
                command: vec![
                    "docker".into(),
                    "run".into(),
                    "-p".into(),
                    "8080:8080".into(),
                    "web:latest".into(),
                ]
            }
        );
    }

    #[test]
    fn multiple_services_with_different_modes() {
        let config = parse(
            r#"
            svc-a:
              mode: replace
              run:
                exec:
                  command: ["node", "a.js"]
            svc-b:
              run:
                exec:
                  command: ["node", "b.js"]
            "#,
        );
        assert_eq!(config.services.len(), 2);
        assert_eq!(config.services["svc-a"].mode, ServiceMode::Replace);
        assert_eq!(config.services["svc-b"].mode, ServiceMode::Mirror);
    }

    #[test]
    fn minimal_service_gets_defaults() {
        let config = parse(
            r#"
            svc:
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        let svc = &config.services["svc"];
        assert_eq!(svc.mode, ServiceMode::Mirror);
        assert_eq!(
            svc.target,
            TargetConfig {
                path: None,
                namespace: None
            }
        );
    }

    #[test]
    fn target_simple_string_form() {
        let config = parse(
            r#"
            svc:
              target: "pod/my-pod/container/main"
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        assert_eq!(
            config.services["svc"]
                .target
                .path
                .as_ref()
                .unwrap()
                .to_string(),
            "pod/my-pod/container/main"
        );
    }

    // -- Error cases --

    #[test]
    fn error_missing_run() {
        let file_config: UpFileConfig = serde_yaml::from_str(
            r#"
            svc:
              mode: steal
            "#,
        )
        .unwrap();
        let mut context = ConfigContext::default();
        let err = file_config.generate_config(&mut context).unwrap_err();
        assert!(
            err.to_string().contains("run"),
            "expected error about missing run, got: {err}"
        );
    }

    #[test]
    fn error_invalid_mode() {
        let result: Result<UpFileConfig, _> = serde_yaml::from_str(
            r#"
            svc:
              mode: bogus
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn error_invalid_target_path() {
        let result: Result<UpFileConfig, _> = serde_yaml::from_str(
            r#"
            svc:
              target: "not-a-valid-target"
              run:
                exec:
                  command: ["echo"]
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn error_invalid_run_variant() {
        let result: Result<UpFileConfig, _> = serde_yaml::from_str(
            r#"
            svc:
              run:
                teleport:
                  command: ["beam", "me", "up"]
            "#,
        );
        assert!(result.is_err());
    }
}
