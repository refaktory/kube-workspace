use anyhow::Context;
use k8s_openapi::api::core::v1::PodSpec;

use crate::AnyError;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Config {
    pub server_port: Option<u16>,

    pub namespace: String,
    pub auto_create_namespace: bool,
    pub users: Vec<User>,

    pub max_home_volume_size: String,
    pub pod_template: PodSpec,
    pub storage_class: Option<String>,
}

impl Config {
    pub fn load_from_env() -> Result<Self, AnyError> {
        let path = std::env::var("KUBE_WORKSPACE_OPERATOR_CONFIG")
            .context("Missing required env var KUBE_WORKSPACE_OPERATOR_CONFIG")?;
        let content = std::fs::read(&path).context("Could not read config file")?;
        let config = serde_json::from_slice(&content).context("Could not deserialize config")?;
        Ok(config)
    }

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

#[derive(serde::Deserialize, Clone, Debug)]
pub struct User {
    pub username: Username,
    pub ssh_public_key: String,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodConfig {
    pub image: String,
    pub max_memory: Option<String>,
    pub max_cpu: Option<String>,
}
