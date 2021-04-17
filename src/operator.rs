//! The Kubernetes operator tooling that handles all interaction with a cluster.

use anyhow::Context;
use k8s_openapi::{
    api::core::v1::{
        Container, ContainerPort, Namespace, Node, PersistentVolumeClaim,
        PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, Pod, PodSpec, Probe,
        ResourceRequirements, Service, ServicePort, ServiceSpec, TCPSocketAction, Volume,
        VolumeMount,
    },
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};
use kube::api::ObjectMeta;

use crate::{
    client::Client,
    config::{self, Config},
    AnyError,
};

#[derive(Clone)]
pub struct Operator {
    config: Config,
    client: Client,
}

impl std::fmt::Debug for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operator()")
    }
}

impl Operator {
    const WORKSPACE_USER_LABEL: &'static str = "workspace-user";

    pub async fn launch(config: Config) -> Result<Self, AnyError> {
        tracing::info!("Operator startup");
        let client = Client::connect().await?;
        let s = Operator { config, client };
        s.bootstrap().await?;
        Ok(s)
    }

    async fn bootstrap(&self) -> Result<(), AnyError> {
        // Check if namespace exists.

        tracing::trace!("Checking namespace {}", self.config.namespace);
        let ns = self.client.namespace_opt(&self.config.namespace).await?;
        if ns.is_none() {
            if self.config.auto_create_namespace {
                tracing::warn!(namespace=%self.config.namespace, "Namespace does not exist. Attempting to create");

                self.client
                    .namespace_create(&Namespace {
                        metadata: ObjectMeta {
                            name: Some(self.config.namespace.clone()),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .await
                    .context("Could not create namespace")?;

                tracing::info!(namespace= %self.config.namespace,"Namespace created");
            } else {
                tracing::error!(namespace=%self.config.namespace, "Namespace does not exist and auto-creation is not enabled. Aborting");
                return Err(anyhow::anyhow!("Bootstrap failed"));
            }
        }
        Ok(())
    }

    fn user_home_volume_name(user: &config::User) -> String {
        format!("workspace-{}", user.username)
    }

    pub async fn ensure_user_home_volume(
        &self,
        user: &config::User,
    ) -> Result<PersistentVolumeClaim, AnyError> {
        let claim_name = Self::user_home_volume_name(&user);

        // First, check if a pod is already running.
        let claim_opt = self
            .client
            .volume_claim_opt(&self.config.namespace, &claim_name)
            .await?;

        if let Some(claim) = claim_opt {
            Ok(claim)
        } else {
            self.create_user_home_volume(user).await
        }
    }

    pub async fn create_user_home_volume(
        &self,
        user: &config::User,
    ) -> Result<PersistentVolumeClaim, AnyError> {
        let ns = &self.config.namespace;
        let claim_name = Self::user_home_volume_name(&user);

        let schema = PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some(claim_name.clone()),
                namespace: Some(ns.to_string()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                storage_class_name: self.config.storage_class.clone(),
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                resources: Some(ResourceRequirements {
                    requests: Some(
                        vec![(
                            "storage".to_string(),
                            Quantity(self.config.max_home_volume_size.clone()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.client
            .volume_claim_create(ns, &schema)
            .await
            .context("Could not create persistent volume for user home directory")
    }

    fn user_service_name(user: &config::User) -> String {
        format!("workspace-{}", user.username)
    }

    // pub async fn get_user_service(&self, user: &config::User) -> Result<Option<Service>, AnyError> {
    //     let name = Self::user_service_name(user);
    //     self.client
    //         .service_opt(&self.config.namespace, &name)
    //         .await
    //         .map_err(Into::into)
    // }

    pub async fn get_user_service_opt(
        &self,
        user: &config::User,
    ) -> Result<Option<Service>, AnyError> {
        let name = Self::user_service_name(user);
        self.client
            .service_opt(&self.config.namespace, &name)
            .await
            .map_err(Into::into)
    }

    pub async fn ensure_user_service(&self, user: &config::User) -> Result<Service, AnyError> {
        if let Some(claim) = self.get_user_service_opt(user).await? {
            Ok(claim)
        } else {
            self.create_user_service(user).await
        }
    }

    async fn create_user_service(&self, user: &config::User) -> Result<Service, AnyError> {
        let name = Self::user_service_name(user);

        let svc = Service {
            metadata: ObjectMeta {
                name: Some(name),
                namespace: Some(self.config.namespace.clone()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                selector: Some(
                    vec![(
                        Self::WORKSPACE_USER_LABEL.to_string(),
                        user.username.clone(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ports: Some(vec![ServicePort {
                    name: Some("ssh".to_string()),
                    port: 22,
                    target_port: Some(IntOrString::String("ssh".into())),
                    ..Default::default()
                }]),
                type_: Some("NodePort".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.client
            .service_create(&self.config.namespace, &svc)
            .await
            .context("Could not create service for user")
    }

    fn user_pod_name(user: &config::User) -> String {
        format!("workspace-{}", user.username)
    }

    // pub async fn get_user_pod(&self, user: &config::User) -> Result<Pod, AnyError> {
    //     let pod_name = Self::user_pod_name(&user);
    //     self.client
    //         .pod(&self.config.namespace, &pod_name)
    //         .await
    //         .map_err(Into::into)
    // }

    pub async fn get_user_pod_opt(&self, user: &config::User) -> Result<Option<Pod>, AnyError> {
        let pod_name = Self::user_pod_name(&user);
        self.client
            .pod_opt(&self.config.namespace, &pod_name)
            .await
            .map_err(Into::into)
    }

    pub async fn ensure_user_pod(
        &self,
        user: &config::User,
        spec: &PodSpec,
    ) -> Result<WorkspaceStatus, AnyError> {
        tracing::debug!(user=%user.username, "Ensuring pod for user");
        self.ensure_user_home_volume(user).await?;
        let service = self.ensure_user_service(user).await?;

        // Try to find running pod.
        let pod_name = Self::user_pod_name(user);

        let pod = if let Some(pod) = self.get_user_pod_opt(user).await? {
            pod
        } else {
            self.create_user_pod(user, spec).await?
        };

        let node_name_opt = pod.spec.as_ref().and_then(|x| x.node_name.as_ref());
        let node = if let Some(name) = node_name_opt {
            Some(self.client.node(name).await?)
        } else {
            None
        };

        tracing::info!(user=%user.username, pod=%pod_name, "Pod for user ensured");

        Ok(WorkspaceStatus {
            phase: WorkspacePhase::from_pod(&pod),
            pod: Some(pod),
            service: Some(service),
            node,
        })
    }

    pub async fn workspace_status(&self, user: &config::User) -> Result<WorkspaceStatus, AnyError> {
        let service = self.get_user_service_opt(user).await?;
        let pod = self.get_user_pod_opt(user).await?;

        match (service.clone(), pod) {
            (Some(service), Some(pod)) => {
                let node =
                    if let Some(node_name) = pod.spec.as_ref().and_then(|x| x.node_name.clone()) {
                        Some(self.client.node(&node_name).await?)
                    } else {
                        None
                    };
                Ok(WorkspaceStatus {
                    service: Some(service),
                    node,
                    phase: WorkspacePhase::from_pod(&pod),
                    pod: Some(pod),
                })
            }
            _ => Ok(WorkspaceStatus {
                phase: WorkspacePhase::NotFound,
                service,
                pod: None,
                node: None,
            }),
        }
    }

    #[tracing::instrument]
    async fn create_user_pod(
        &self,
        user: &config::User,
        spec_template: &PodSpec,
    ) -> Result<Pod, AnyError> {
        let ns = &self.config.namespace;
        let pod_name = Self::user_pod_name(&user);

        tracing::debug!(user=%user.username, pod_name=%pod_name, "Creating user pod");

        let home_volume = self.ensure_user_home_volume(user).await?;

        // Create the pod.

        let command = vec![
            "bash".to_string(),
            "-c".to_string(),
            vec![
                "apt-get update",
                "apt-get install -y openssh-server",
                &format!(
                    "adduser --gecos \"\" --no-create-home --disabled-password {}",
                    user.username
                ),
                &format!("mkdir -p /home/{}/.ssh", user.username),
                &format!(
                    "echo '{}' > /home/{}/.ssh/authorized_keys",
                    user.ssh_public_key, user.username
                ),
                // Ensure correct permissions.
                &format!("chown {u}:{u} /home/{u}", u = user.username),
                &format!("chown {u}:{u} /home/{u}/.ssh", u = user.username),
                &format!("chmod 755 /home/{}", user.username),
                &format!("chmod 755 /home/{}/.ssh", user.username),
                &format!("chmod 644 /home/{}/.ssh/authorized_keys", user.username),
                // // Must create run dir for sshd.
                // "/usr/sbin/sshd -d",
                // "mkdir -p /run/sshd",
                "service ssh start",
                "sleep infinity",
            ]
            .join(" && "),
        ];

        let spec = {
            let mut spec = spec_template.clone();
            let main_container = if let Some(container) = spec.containers.get_mut(0) {
                container
            } else {
                spec.containers.push(Container {
                    ..Default::default()
                });
                spec.containers.get_mut(0).unwrap()
            };

            main_container.image.get_or_insert("ubuntu".into());
            main_container.name = "workspace".to_string();
            main_container.command = Some(command);

            main_container
                .volume_mounts
                .get_or_insert(Vec::new())
                .push(VolumeMount {
                    mount_path: format!("/home/{}", user.username),
                    name: "home".to_string(),
                    ..Default::default()
                });

            main_container
                .ports
                .get_or_insert(Vec::new())
                .push(ContainerPort {
                    container_port: 22,
                    name: Some("ssh".into()),
                    ..Default::default()
                });

            main_container.readiness_probe = Some(Probe {
                tcp_socket: Some(TCPSocketAction {
                    host: None,
                    port: IntOrString::String("ssh".into()),
                }),
                initial_delay_seconds: Some(60),
                period_seconds: Some(30),
                timeout_seconds: Some(3),
                ..Default::default()
            });

            spec.volumes.get_or_insert(Vec::new()).push(Volume {
                name: "home".to_string(),
                persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                    claim_name: home_volume.metadata.name.as_ref().unwrap().clone(),
                    ..Default::default()
                }),
                ..Default::default()
            });

            spec
        };

        let schema = Pod {
            metadata: ObjectMeta {
                name: Some(pod_name),
                namespace: Some(ns.to_string()),
                labels: Some(
                    vec![(
                        Self::WORKSPACE_USER_LABEL.to_string(),
                        user.username.clone(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            spec: Some(spec),
            status: None,
        };

        let pod = self
            .client
            .pod_create(ns, &schema)
            .await
            .context("Could not create pod for user")?;
        tracing::info!(user=%user.username, "user_pod_created");
        Ok(pod)
    }

    pub async fn user_pod_shutdown(&self, user: &config::User) -> Result<(), AnyError> {
        let name = Self::user_pod_name(user);
        tracing::debug!(pod=%name, user=%user.username, "deleting user pod");
        self.client
            .pod_delete(&self.config.namespace, &name)
            .await?;
        // Delete the service.
        self.client
            .service_delete(&self.config.namespace, &Self::user_service_name(&user))
            .await?;
        tracing::info!(user=%user.username, pod=%name, "user pod deleted");
        Ok(())
    }

    /// Get a reference to the operator's config.
    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// The current status phase of a user workspace.
#[derive(serde::Serialize, Clone, Debug)]
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
            Some("Running") if pod_containers_ready(pod) => Self::Ready,
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
        self.node.as_ref().and_then(node_ip)
    }

    /// Get the SSH port for the pod.
    pub fn ssh_port(&self) -> Option<i32> {
        self.service.as_ref().and_then(service_get_nodeport)
    }
}

/// Extract the NodePort of a `Service`.
pub fn service_get_nodeport(svc: &Service) -> Option<i32> {
    svc.spec.as_ref()?.ports.as_ref()?.first()?.node_port
}

/// Determine if all containers of a `Pod` are ready.
/// Ready means that they are up and running and are passing the readinessCheck
/// if one is configured.
fn pod_containers_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|x| x.container_statuses.as_ref())
        .map(|s| s.iter().all(|x| x.ready))
        .unwrap_or_default()
}

pub fn node_ip(node: &Node) -> Option<String> {
    node.status
        .as_ref()?
        .addresses
        .as_ref()?
        .iter()
        .find(|addr| addr.type_ == "InternalIP")
        .map(|addr| addr.address.clone())
}
