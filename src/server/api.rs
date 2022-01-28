use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

pub(super) async fn run_query(
    server: &super::Server,
    query: &Query,
) -> Result<QueryOutput, anyhow::Error> {
    let op = &server.operator;

    tracing::trace!(?query, "Handling API request");
    match query {
        Query::PodStart(create) => {
            let config = op.config();

            let user = op
                .config()
                .verify_user(&create.username, &create.ssh_public_key)?;
            let status = op.ensure_user_pod(user, &config.pod_template).await?;

            let addr = status.public_address();
            let port = status.ssh_port();

            let ssh_address = addr
                .zip(port)
                .map(|(address, port)| SshAddress { address, port });

            let info = status.pod.as_ref().map(WorkspaceInfo::from_pod);

            Ok(QueryOutput::PodStart(WorkspaceStatus {
                username: user.username.clone(),
                phase: status.phase,
                ssh_address,
                info,
            }))
        }
        Query::PodStatus(req) => {
            let user = op
                .config()
                .verify_user(&req.username, &req.ssh_public_key)?;
            let status = op.workspace_status(user).await?;

            let addr = status.public_address();
            let port = status.ssh_port();
            let ssh_address = addr
                .zip(port)
                .map(|(address, port)| SshAddress { address, port });
            let info = status.pod.as_ref().map(WorkspaceInfo::from_pod);

            Ok(QueryOutput::PodStatus(WorkspaceStatus {
                username: user.username.clone(),
                phase: status.phase,
                ssh_address,
                info,
            }))
        }
        Query::PodStop(req) => {
            let user = op
                .config()
                .verify_user(&req.username, &req.ssh_public_key)?;
            if op.get_user_pod_opt(user).await?.is_some() {
                op.user_pod_shutdown(user).await?;
            }
            Ok(QueryOutput::PodStop {})
        }
    }
}

#[derive(serde::Deserialize, Clone, Debug)]
// TODO: remove allow if new variants are added.
#[allow(clippy::enum_variant_names)]
pub enum Query {
    PodStart(PodStartRequest),
    PodStatus(PodStatusRequest),
    PodStop(PodStopRequest),
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodStartRequest {
    pub username: String,
    pub ssh_public_key: String,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodStatusRequest {
    pub username: String,
    pub ssh_public_key: String,
}
#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodStopRequest {
    pub username: String,
    pub ssh_public_key: String,
}
#[derive(serde::Serialize, Clone, Debug)]
pub struct SshAddress {
    pub address: String,
    pub port: i32,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct WorkspaceInfo {
    /// The OCI image the container is running.
    pub image: String,
    pub memory_limit: Option<Quantity>,
    pub cpu_limit: Option<Quantity>,
}

impl WorkspaceInfo {
    pub fn from_pod(pod: &k8s_openapi::api::core::v1::Pod) -> Self {
        let container = pod.spec.as_ref().and_then(|s| s.containers.first());

        let limits = container
            .and_then(|c| c.resources.as_ref())
            .and_then(|r| r.limits.as_ref());

        let image = container
            .and_then(|c| c.image.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        let memory_limit = limits.and_then(|l| l.get("memory").cloned());
        let cpu_limit = limits.and_then(|l| l.get("cpu").cloned());

        Self {
            image,
            memory_limit,
            cpu_limit,
        }
    }
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct WorkspaceStatus {
    pub username: String,
    pub phase: crate::operator::WorkspacePhase,
    pub ssh_address: Option<SshAddress>,
    pub info: Option<WorkspaceInfo>,
}

#[derive(serde::Serialize, Clone, Debug)]
// TODO: remove allow if new variants are added.
#[allow(clippy::enum_variant_names)]
pub enum QueryOutput {
    PodStart(WorkspaceStatus),
    PodStatus(WorkspaceStatus),
    PodStop {},
}

#[derive(serde::Serialize, Clone, Debug)]
pub enum ApiResult<T> {
    Ok(T),
    Error { message: String },
}
