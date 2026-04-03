use std::{collections::HashMap, fmt, marker::PhantomData, str::FromStr};

/// Configuration types for `mirrord-up.yaml`.
///
/// Uses the two-layer config pattern from mirrord-config: file config structs (with
/// `Option<T>` fields, for flexible deserialization) are resolved into runtime config structs
/// (with concrete types and defaults applied) via `MirrordConfig::generate_config`.
use mirrord_config::{
    config::ConfigError,
    feature::{
        env::EnvConfig,
        network::incoming::http_filter::{HttpFilterConfig, HttpFilterFileConfig},
    },
    target::Target,
};
use serde::{Deserialize, Deserializer, Serialize, de};

/// Incoming traffic mode for a service.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceMode {
    #[default]
    Mirror,
    Replace,
    Steal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RunType {
    Exec,
    Container,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct RunConfig {
    r#type: RunType,
    command: Vec<String>,
}

/// Default settings applied to all services.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DefaultsConfig {
    pub accept_invalid_certificates: bool,
    pub operator: bool,
    pub telemetry: bool,
}

pub fn string_or_struct_option<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = ConfigError>,
    D: Deserializer<'de>,
{
    // This is a Visitor that forwards string types to T's `FromStr` impl and
    // forwards map types to T's `Deserialize` impl. The `PhantomData` is to
    // keep the compiler from complaining about T being an unused generic type
    // parameter. We need T in order to know the Value type for the Visitor
    // impl.
    struct StringOrStruct<T>(PhantomData<fn() -> T>);

    impl<'de, T> de::Visitor<'de> for StringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = ConfigError>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or map")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            FromStr::from_str(value).map_err(|err| de::Error::custom(err))
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            // `MapAccessDeserializer` is a wrapper that turns a `MapAccess`
            // into a `Deserializer`, allowing it to be used as the input to T's
            // `Deserialize` implementation. T then deserializes itself using
            // the entries from the map visitor.
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map)).map(T::into)
        }
    }

    deserializer.deserialize_any(StringOrStruct(PhantomData))
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct TargetConfig {
    #[serde(deserialize_with = "string_or_struct_option")]
    // #[schemars(schema_with = "make_simple_target_custom_schema")]
    pub path: Target,
    pub namespace: Option<String>,
}

/// Per-service configuration.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ServiceConfig {
    pub target: TargetConfig,
    pub env: EnvConfig,
    pub mode: ServiceMode,
    pub http_filter: Option<HttpFilterConfig>,
    pub run: RunConfig,
}

/// Resolved top-level `mirrord-up.yaml` configuration.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UpConfig {
    pub defaults: DefaultsConfig,
    pub services: HashMap<String, ServiceConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse YAML into UpConfig via the two-layer config system.
    fn parse(yaml: &str) -> UpConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn defaults_applied_when_omitted() {
        let config = parse(
            r#"
            services:
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
            services:
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
            services:
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
            services:
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
            services:
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
            services:
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

    // #[test]
    // fn error_missing_run() {
    //     let file_config: UpConfig = serde_yaml::from_str(
    //         r#"
    //         services:
    //           svc:
    //             mode: steal
    //         "#,
    //     )
    //     .unwrap();
    //     let mut context = ConfigContext::default();
    //     let err = file_config.generate_config(&mut context).unwrap_err();
    //     assert!(
    //         err.to_string().contains("run"),
    //         "expected error about missing run, got: {err}"
    //     );
    // }

    // #[test]
    // fn error_invalid_mode() {
    //     let result: Result<UpConfig, _> = serde_yaml::from_str(
    //         r#"
    //         services:
    //           svc:
    //             mode: bogus
    //             run:
    //               exec:
    //                 command: ["echo"]
    //         "#,
    //     );
    //     assert!(result.is_err());
    // }

    // #[test]
    // fn error_invalid_target_path() {
    //     let result: Result<UpConfig, _> = serde_yaml::from_str(
    //         r#"
    //         services:
    //           svc:
    //             target: "not-a-valid-target"
    //             run:
    //               exec:
    //                 command: ["echo"]
    //         "#,
    //     );
    //     assert!(result.is_err());
    // }

    // #[test]
    // fn error_invalid_run_variant() {
    //     let file_config: UpConfig = serde_yaml::from_str(
    //         r#"
    //         services:
    //           svc:
    //             run:
    //               teleport:
    //                 command: ["beam", "me", "up"]
    //         "#,
    //     )
    //     assert!(
    //         err.to_string().contains("run"),
    //         "expected error about run, got: {err}"
    //     );
    // }
}
