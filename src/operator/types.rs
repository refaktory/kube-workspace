use k8s_openapi::api::core::v1::{Node, Pod, Service};

use crate::client;

/// The current status phase of a user workspace.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
pub enum WorkspacePhase {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "terminating")]
    Terminating,
    #[serde(rename = "unknown")]
    Unknown,
}

impl WorkspacePhase {
    pub fn from_pod(pod: &Pod) -> Self {
        let phase = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_ref())
            .map(|x| x.as_str());
        match phase {
            _ if pod.metadata.deletion_timestamp.is_some() => Self::Terminating,
            Some("Pending") => Self::Starting,
            Some("Running") if client::pod_containers_ready(pod) => Self::Ready,
            Some("Running") => Self::Starting,
            Some("Succeeded") => Self::Terminating,
            Some("Failed") => Self::Terminating,
            Some("Unknown") => Self::Unknown,
            Some(other) => {
                tracing::warn!(
                    stauts=%other,
                    "Internal error: unhandled pod status '{}' \
                    - please file a bug report",
                    other
                );
                Self::Unknown
            }
            None => Self::Unknown,
        }
    }
}

#[derive(Debug)]
pub struct WorkspaceStatus {
    pub phase: WorkspacePhase,
    pub service: Option<Service>,
    pub pod: Option<Pod>,
    pub node: Option<Node>,
}

impl WorkspaceStatus {
    /// Get the public address where the pod SSH can be reached.
    /// Can be an IP or a hostname.
    pub fn public_address(&self) -> Option<String> {
        self.node.as_ref().and_then(client::node_ip)
    }

    /// Get the SSH port for the pod.
    pub fn ssh_port(&self) -> Option<i32> {
        self.service.as_ref().and_then(client::service_get_nodeport)
    }
}
