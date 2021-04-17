//! Application configuration and parsing.

use anyhow::Context;
use k8s_openapi::api::core::v1::PodSpec;

use crate::AnyError;

/// Application configuration.
///
/// Can be parsed from a config.json config file.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct Config {
    /// Port where the API server should run.
    pub server_port: Option<u16>,

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
}

impl Config {
    /// Load application configuration.
    /// Respects various environment flags.
    pub fn load_from_env() -> Result<Self, AnyError> {
        let path = std::env::var("KUBE_WORKSPACE_OPERATOR_CONFIG")
            .context("Missing required env var KUBE_WORKSPACE_OPERATOR_CONFIG")?;
        let content = std::fs::read(&path).context("Could not read config file")?;
        let config = serde_json::from_slice(&content).context("Could not deserialize config")?;
        Ok(config)
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
}

pub type Username = String;

/// A single configured/whitelisted user account.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct User {
    pub username: Username,
    pub ssh_public_key: String,
}
