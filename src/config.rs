//! Application configuration and parsing.

use std::collections::HashMap;

use anyhow::{bail, Context};
use k8s_openapi::api::core::v1::PodSpec;

use crate::AnyError;

const ENV_VAR_CONFIG_PATH: &'static str = "KUBE_WORKSPACE_OPERATOR_CONFIG";

/// External configuration source with most values optional.
///
/// Can be parsed from a config.json config file or from env vars.
#[derive(serde::Deserialize, Default, Debug)]
pub struct ConfigSource {
    /// Port where the API server should run.
    pub server_port: Option<u16>,

    /// The namespace where user volumes and workspace pods are created.
    pub namespace: Option<String>,
    /// If true, the configured namespace is automatically created if it does
    /// not exist.
    pub auto_create_namespace: Option<bool>,
    /// The user whitelist that is allowed to create containers.
    #[serde(default)]
    pub users: Vec<User>,

    /// Maximum size for user /home volumes.
    /// Also used as the default value.
    pub max_home_volume_size: Option<String>,
    /// Template for workspace pods.
    /// This template will be merged with other configuration when new
    /// workspaces are created.
    pub pod_template: Option<PodSpec>,
    /// The Kubernetes storage class to for the user /home volumes.
    pub storage_class: Option<String>,

    pub auto_shutdown: Option<AutoShutdown>,
}

impl ConfigSource {
    /// Load application configuration.
    /// Respects various environment flags.
    pub fn load_from_env() -> Result<Config, AnyError> {
        let vars: HashMap<String, String> = std::env::vars().collect();

        let file_config = if let Some(path) = vars.get(ENV_VAR_CONFIG_PATH) {
            tracing::trace!(path=%path, "loading config file");
            let content = std::fs::read(&path).context("Could not read config file")?;
            serde_json::from_slice(&content).context("Could not deserialize config")?
        } else {
            ConfigSource::default()
        };

        // TODO: parse individual settings from individual env vars
        // ( KUBE_WORKSPACE_* )
        let server_port = file_config.server_port;
        let namespace = file_config.namespace;
        let auto_create_namespace = file_config.auto_create_namespace;
        let users = file_config.users;
        let max_home_volume_size = file_config.max_home_volume_size;
        let pod_template = file_config.pod_template;
        let storage_class = file_config.storage_class;
        let auto_shutdown = file_config.auto_shutdown;

        let source = Self {
            server_port,
            namespace,
            auto_create_namespace,
            users,
            max_home_volume_size,
            pod_template,
            storage_class,
            auto_shutdown,
        };

        let config = source.into_config();
        config.validate()?;
        Ok(config)
    }

    /// Convert into a [`Config`] by setting default values.
    fn into_config(self) -> Config {
        Config {
            server_port: self.server_port.unwrap_or(8080),
            namespace: self
                .namespace
                .map(|x| x.trim().to_string())
                .unwrap_or("kube-workspaces".to_string()),
            auto_create_namespace: self.auto_create_namespace.unwrap_or(true),
            users: self.users,
            max_home_volume_size: self.max_home_volume_size.unwrap_or("10Gi".to_string()),
            pod_template: self.pod_template.unwrap_or(PodSpec {
                ..Default::default()
            }),
            storage_class: self.storage_class,
            auto_shutdown: self.auto_shutdown.unwrap_or(AutoShutdown {
                enable: false,
                cpu_usage: None,
                tcp_idle: None,
            }),
        }
    }
}

/// Application configuration.
///
/// Can be parsed from a config.json config file.
#[derive(Clone, Debug)]
pub struct Config {
    /// Port where the API server should run.
    pub server_port: u16,

    /// The namespace where user volumes and workspace pods are created.
    pub namespace: String,
    /// If true, the configured namespace is automatically created if it does
    /// not exist.
    pub auto_create_namespace: bool,
    /// The user whitelist that is allowed to create containers.
    pub users: Vec<User>,

    /// Maximum size for user /home volumes.
    /// Also used as the default value.
    pub max_home_volume_size: String,
    /// Template for workspace pods.
    /// This template will be merged with other configuration when new
    /// workspaces are created.
    pub pod_template: PodSpec,
    /// The Kubernetes storage class to for the user /home volumes.
    pub storage_class: Option<String>,

    pub auto_shutdown: AutoShutdown,
}

impl Config {
    /// Check if autoshutdown is enabled.
    pub fn autoshutdown_enabled(&self) -> bool {
        self.auto_shutdown.enable
            && (self.auto_shutdown.tcp_idle.is_some() || self.auto_shutdown.cpu_usage.is_some())
    }

    /// Verify that a username and SSH public key pair are in the configured
    /// user whitelist.
    pub fn verify_user(&self, username: &str, ssh_key: &str) -> Result<&User, AnyError> {
        let user = self
            .users
            .iter()
            .find(|u| u.username == username)
            .context("Username not found")?;

        if user.ssh_public_key.trim() != ssh_key.trim() {
            Err(anyhow::anyhow!("Invalid/unknown ssh public key"))
        } else {
            Ok(user)
        }
    }

    fn validate(&self) -> Result<(), anyhow::Error> {
        if self.namespace.trim() != &self.namespace {
            bail!(
                "Invalid namespace '{}': leading or trailing spaces",
                self.namespace
            );
        }
        if self.namespace == "" {
            bail!("Namespace may not be an empty string");
        }

        Ok(())
    }
}

/// Automatic container shutdown configuration.
#[derive(serde::Deserialize, Clone, Debug, Default)]
pub struct AutoShutdown {
    pub enable: bool,
    pub cpu_usage: Option<CpuIdleAutoShutown>,
    pub tcp_idle: Option<TcpIdleAutoShutdown>,
}

/// Automatic container shutdown configuration.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct CpuIdleAutoShutown {
    /// Minimum time that the pod needs to be below the specified CPU usage
    /// threshold.
    /// Format: all formats supported by the humantime crate.
    /// EG: "2 hours", "1d", "5 hours 20m"
    #[serde(with = "humantime_serde")]
    pub minimum_idle_time: std::time::Duration,
    /// CPU threshold that is considered idle.
    /// The number here corresponds to the normalized CPU usage metric in
    /// Kubernetes, which is also used for resource limits.
    /// See https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/#meaning-of-cpu.
    pub cpu_threshold: u64,
}

/// Configure auto-shutdown of containers when no tcp connections are detected.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct TcpIdleAutoShutdown {
    /// Minimum number of seconds before idle shutdown takes effect.
    /// Format: all formats supported by the humantime crate.
    /// EG: "2 hours", "1d", "5 hours 20m"
    #[serde(with = "humantime_serde")]
    pub minimum_idle_time: std::time::Duration,
    /// TCP ports to ignore.
    pub ignored_ports: Vec<u16>,
}

pub type Username = String;

/// A single configured/whitelisted user account.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct User {
    pub username: Username,
    pub ssh_public_key: String,
}
