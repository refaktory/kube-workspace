//! The Kubernetes operator that handles all interaction with a cluster.

mod autoshutdown;
mod types;

use std::{collections::BTreeMap, iter::FromIterator, sync::Arc};

pub use self::types::WorkspacePhase;
use self::types::WorkspaceStatus;

use anyhow::{anyhow, Context};
use client::PodMetrics;
use k8s_openapi::{
    api::core::v1::{
        Container, ContainerPort, Namespace, PersistentVolumeClaim, PersistentVolumeClaimSpec,
        PersistentVolumeClaimVolumeSource, Pod, PodSpec, Probe, ResourceRequirements, Service,
        ServicePort, ServiceSpec, TCPSocketAction, Volume, VolumeMount,
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, util::intstr::IntOrString,
    },
};
use kube::api::ObjectMeta;
use prometheus_client::metrics::gauge::Gauge;

use crate::{
    client::{self, Client},
    config::{self, Config},
    AnyError,
};

#[derive(Clone)]
pub struct Operator(Arc<State>);

struct State {
    config: Config,
    client: Client,
    metrics: OperatorMetrics,
}

#[derive(Clone, Default)]
pub struct OperatorMetrics {
    pub configuration_errors: Gauge,
    pub workspace_available_count: Gauge,
    pub workspace_unavailable_count: Gauge,
}

impl std::fmt::Debug for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operator()")
    }
}

impl Operator {
    const WORKSPACE_USER_LABEL: &'static str = "workspace-user";
    const WORKSPACE_POD_LABEL: &'static str = "workspace-pod";
    const WORKSPACE_POD_LABEL_VALUE: &'static str = "true";
    const POD_MAIN_CONTAINER_NAME: &'static str = "workspace";

    /// Build the pod label applied to all workspace pods.
    pub fn workspace_pod_label() -> (String, String) {
        (
            Self::WORKSPACE_POD_LABEL.to_string(),
            Self::WORKSPACE_POD_LABEL_VALUE.to_string(),
        )
    }

    /// Get a reference to the operator's config.
    #[inline]
    pub fn config(&self) -> &Config {
        &self.0.config
    }

    fn namespace(&self) -> &str {
        &self.0.config.namespace
    }

    /// Get a reference to the operator's config.
    #[inline]
    fn client(&self) -> &Client {
        &self.0.client
    }

    /// Get a reference to the operator's config.
    #[inline]
    pub fn metrics(&self) -> &OperatorMetrics {
        &self.0.metrics
    }

    pub async fn launch(config: Config) -> Result<Self, AnyError> {
        tracing::info!("operator startup");
        let client = Client::connect().await?;

        let s = Operator(Arc::new(State {
            config,
            client,
            metrics: OperatorMetrics::default(),
        }));
        s.ensure_namespace().await?;

        // Spawn the main check loop of the operator.
        tokio::task::spawn(s.clone().run_loop());
        Ok(s)
    }

    /// Main loop of the operator that performs recurring checks.
    async fn run_loop(self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(err) = self.run_checks().await {
                tracing::error!(?err, "pod check failed");
            }
        }
    }

    async fn run_checks(&self) -> Result<(), AnyError> {
        tracing::trace!("running check job");
        // TODO: mark operator as unhealthy if namespace could not be ensured.
        self.ensure_namespace().await?;
        self.check_pods().await?;

        if let Some(conf) = &self.config().prometheus_exporter {
            if conf.auto_register_operator_service_monitor {
                self.ensure_prometheus_operator_service_monitor(conf)
                    .await?;
            }
        }
        Ok(())
    }

    fn default_labels() -> BTreeMap<String, String> {
        BTreeMap::from_iter(vec![(
            "app.kubernetes.io/managed-by".to_string(),
            "kube-workspace-operator".to_string(),
        )])
    }

    async fn ensure_prometheus_operator_service_monitor(
        &self,
        _config: &crate::config::ConfigPrometheusExporter,
    ) -> Result<(), anyhow::Error> {
        tracing::trace!("checking prometheus operator service monitor");

        let client = self.client();

        let crd_name = "servicemonitors.monitoring.coreos.com";
        let service_monitor_name = "kube-workspace-prometheus-operator-servicemonitor";

        let mon = crate::prometheus::ServiceMonitor {
            metadata: ObjectMeta {
                name: Some(service_monitor_name.to_string()),
                labels: Some(Self::default_labels()),
                ..Default::default()
            },
            spec: crate::prometheus::ServiceMonitorSpec {
                join_label: None,
                selector: LabelSelector {
                    match_labels: Some(BTreeMap::from_iter(vec![(
                        "app.kubernetes.io/name".to_string(),
                        "kube-workspace-operator".to_string(),
                    )])),
                    match_expressions: None,
                },
                endpoints: vec![crate::prometheus::Endpoint {
                    port: Some("prometheus".to_string()),
                    path: None,
                }],
            },
        };

        let existing = client
            .prometheus_servicemonitor_opt(self.namespace(), service_monitor_name)
            .await?;
        if let Some(_old) = existing {
            // TODO: check if config has changed and re-apply it.

            tracing::trace!("prometheus-operator ServiceMonitor already exists");

            return Ok(());
        }

        // Check if CRD exists.
        let crd = client.custom_resource_definition_by_name(crd_name).await?;
        if crd.is_none() {
            tracing::debug!(
                "not creating prometheus-operator ServiceMonitor - ServiceMonitor CRD not found"
            );
            return Ok(());
        }

        client
            .prometheus_servicemonitor_create(self.namespace(), mon)
            .await
            .context("Could not create prometheus-operator ServiceMonitor")?;
        tracing::info!("prometheus-operator ServiceMonitor created");
        Ok(())
    }

    /// Check the currently running pods.
    /// If auto shutdown is enabled, check status and shutdown down if approrpriate.
    async fn check_pods(&self) -> Result<(), AnyError> {
        let pod_label = Self::workspace_pod_label();

        let pods = self
            .client()
            .pods_all(self.namespace(), Some(pod_label))
            .await?;
        let pod_metrics = self
            .client()
            .pod_metrics_list_all(self.namespace())
            .await
            .unwrap_or_else(|error| {
                // The metrics API is optional and depends on a metrics-server
                // deployment.
                // Handle this gracefully by not propagating the error but just
                // logging a warning.
                // TODO: separate startup manual check for the pod metrics API
                //  (for better error messages)
                tracing::warn!(
                    ?error,
                    "could not obtain pod metrics - is the pod metrics API installed?"
                );
                Vec::new()
            });

        let mut available_count = 0;
        let mut unavailable_count = 0;

        for pod in pods {
            let metrics = pod_metrics
                .iter()
                .find(|metrics| metrics.metadata.name == pod.metadata.name);

            match WorkspacePhase::from_pod(&pod) {
                WorkspacePhase::Starting => {
                    unavailable_count += 1;
                }
                WorkspacePhase::Ready => {
                    available_count += 1;
                }
                WorkspacePhase::Terminating
                | WorkspacePhase::Unknown
                | WorkspacePhase::NotFound => {}
            }

            if self.config().autoshutdown_enabled() {
                if let Err(err) = self.process_pod_autoshutdown(pod, metrics.cloned()).await {
                    tracing::error!(error=?err, "Could not process pod autoshutdown");
                }
            }
        }

        self.0
            .metrics
            .workspace_available_count
            .set(available_count);
        self.0
            .metrics
            .workspace_unavailable_count
            .set(unavailable_count);

        Ok(())
    }

    async fn process_pod_autoshutdown(
        &self,
        pod: Pod,
        metrics_opt: Option<PodMetrics>,
    ) -> Result<(), AnyError> {
        let pod_name = client::pod_name(&pod);
        let annotations = self.analyze_pod_autoshutdown(&pod, metrics_opt).await?;

        if annotations.should_shutdown(&self.config().auto_shutdown) {
            tracing::trace!(
                ?pod,
                ?annotations,
                "shutting down workspace pod due to auto shutdown"
            );
            self.client()
                .pod_delete(self.namespace(), client::pod_name(&pod))
                .await?;
            tracing::info!(pod=%pod_name, "Workspace pod shut down due to autoshutdown");
        } else {
            // Update annotations.
            tracing::trace!(?pod, ?annotations, "Updating pod autoshutdown annotations");
            let (patch, params) = annotations.to_patch();
            self.client()
                .pod_patch(&self.config().namespace, pod_name, &patch, &params)
                .await?;
        }

        Ok(())
    }

    /// Analyze auto-shutdown conditions for a pod.
    async fn analyze_pod_autoshutdown(
        &self,
        pod: &Pod,
        metrics_opt: Option<PodMetrics>,
    ) -> Result<PodMetricsAnnotion, AnyError> {
        let pod_name = pod
            .metadata
            .name
            .as_ref()
            .ok_or_else(|| anyhow!("Pod has no name"))?;

        let now = chrono::Utc::now();

        let mut annotations = PodMetricsAnnotion::from_pod(pod).unwrap_or_default();

        // If the last check was too long ago, we can't trust the metrics and need to start over.
        if let Some(last) = annotations.last_idle_check {
            if now.signed_duration_since(last).to_std()? > std::time::Duration::from_secs(60 * 5) {
                // Last check to old, so reset metrics.
                annotations.cpu_idle_since = None;
                annotations.network_idle_since = None;
            }
        }

        let cfg = &self.config().auto_shutdown;
        let cpu_is_idle = if let Some((cpu, metrics)) = cfg.cpu_usage.as_ref().zip(metrics_opt) {
            client::pod_metrics_total_cpu(&metrics)? > cpu.cpu_threshold as i64
        } else {
            false
        };

        let active_connections = self
            .pod_active_tcp_connections(pod_name)
            .await
            .context("Could not determine active TCP connections of pod")?;
        let network_is_idle = active_connections == 0;

        let new_annotations = PodMetricsAnnotion {
            last_idle_check: Some(now),
            cpu_idle_since: if cpu_is_idle {
                annotations.cpu_idle_since.or(Some(now))
            } else {
                None
            },
            network_idle_since: if network_is_idle {
                annotations.network_idle_since.or(Some(now))
            } else {
                None
            },
        };
        Ok(new_annotations)
    }

    async fn pod_active_tcp_connections(&self, pod_name: &str) -> Result<usize, AnyError> {
        let stdout = self
            .client()
            .pod_exec_stdout(
                &self.config().namespace,
                pod_name,
                Self::POD_MAIN_CONTAINER_NAME,
                vec!["ss", "--tcp", "--oneline", "--no-header"],
            )
            .await?;

        Ok(stdout.trim().lines().count())
    }

    /// ensure that the specified namespace exists.
    async fn ensure_namespace(&self) -> Result<(), AnyError> {
        // Check if namespace exists.
        let config = self.config();

        tracing::trace!("Checking namespace {}", config.namespace);
        let ns = self.client().namespace_opt(&config.namespace).await?;
        if ns.is_none() {
            if config.auto_create_namespace {
                tracing::warn!(namespace=%config.namespace, "Namespace does not exist. Attempting to create");

                self.client()
                    .namespace_create(&Namespace {
                        metadata: ObjectMeta {
                            name: Some(config.namespace.clone()),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .await
                    .context("Could not create namespace")?;

                tracing::info!(namespace= %config.namespace,"Namespace created");
            } else {
                tracing::error!(namespace=%config.namespace, "Namespace does not exist and auto-creation is not enabled. Aborting");
                return Err(anyhow::anyhow!("Bootstrap failed"));
            }
        }
        tracing::debug!(namespace=%config.namespace, "namespace ready");
        Ok(())
    }

    fn user_home_volume_name(user: &config::User) -> String {
        format!("workspace-{}", user.username)
    }

    pub async fn ensure_user_home_volume(
        &self,
        user: &config::User,
    ) -> Result<PersistentVolumeClaim, AnyError> {
        let claim_name = Self::user_home_volume_name(user);

        // First, check if a pod is already running.
        let claim_opt = self
            .client()
            .volume_claim_opt(&self.config().namespace, &claim_name)
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
        let ns = &self.config().namespace;
        let claim_name = Self::user_home_volume_name(user);

        let schema = PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some(claim_name.clone()),
                namespace: Some(ns.to_string()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                storage_class_name: self.config().storage_class.clone(),
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                resources: Some(ResourceRequirements {
                    requests: Some(
                        vec![(
                            "storage".to_string(),
                            Quantity(self.config().max_home_volume_size.clone()),
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

        self.client()
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
        self.client()
            .service_opt(self.namespace(), &name)
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
                namespace: Some(self.namespace().to_string()),
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

        self.client()
            .service_create(self.namespace(), &svc)
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
        let pod_name = Self::user_pod_name(user);
        self.client()
            .pod_opt(self.namespace(), &pod_name)
            .await
            .map_err(Into::into)
    }

    #[tracing::instrument]
    async fn create_user_pod(
        &self,
        user: &config::User,
        spec_template: &PodSpec,
    ) -> Result<Pod, AnyError> {
        let ns = self.namespace();
        let pod_name = Self::user_pod_name(user);

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
            main_container.name = Self::POD_MAIN_CONTAINER_NAME.to_string();
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
                    vec![
                        (
                            Self::WORKSPACE_POD_LABEL.to_string(),
                            Self::WORKSPACE_POD_LABEL_VALUE.to_string(),
                        ),
                        (
                            Self::WORKSPACE_USER_LABEL.to_string(),
                            user.username.clone(),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            spec: Some(spec),
            status: None,
        };

        let pod = self
            .client()
            .pod_create(ns, &schema)
            .await
            .context("Could not create pod for user")?;
        tracing::info!(user=%user.username, "user_pod_created");
        Ok(pod)
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
            Some(self.client().node(name).await?)
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
                        Some(self.client().node(&node_name).await?)
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

    pub async fn user_pod_shutdown(&self, user: &config::User) -> Result<(), AnyError> {
        let name = Self::user_pod_name(user);
        tracing::debug!(pod=%name, user=%user.username, "deleting user pod");
        self.client().pod_delete(self.namespace(), &name).await?;
        // Delete the service.
        self.client()
            .service_delete(self.namespace(), &Self::user_service_name(user))
            .await?;
        tracing::info!(user=%user.username, pod=%name, "user pod deleted");
        Ok(())
    }
}

/// Custom annotation data applied to pods.
/// Used for idle time tracking.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
struct PodMetricsAnnotion {
    last_idle_check: Option<chrono::DateTime<chrono::Utc>>,
    cpu_idle_since: Option<chrono::DateTime<chrono::Utc>>,
    network_idle_since: Option<chrono::DateTime<chrono::Utc>>,
}

impl PodMetricsAnnotion {
    const ANNOTATION_KEY: &'static str = "kube-workspaces.foundational.cc/pod-data";

    /// Extract the annotation from a Pod.
    fn from_pod(pod: &Pod) -> Option<Self> {
        pod.metadata
            .annotations
            .as_ref()
            .and_then(|x| x.get(Self::ANNOTATION_KEY))
            .and_then(|raw| serde_json::from_str(raw).ok())
    }

    /// Convert into a patch that can be applied with [kube::api::Api::patch].
    fn to_patch(&self) -> (kube::api::Patch<Pod>, kube::api::PatchParams) {
        let patch = Pod {
            metadata: kube::api::ObjectMeta {
                annotations: Some(
                    vec![(
                        Self::ANNOTATION_KEY.to_string(),
                        serde_json::to_string(&self).unwrap(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        (
            kube::api::Patch::Apply(patch),
            kube::api::PatchParams::apply("kube-workspaces.foundational.cc"),
        )
    }

    /// Compare idle times against the shutdown config and determine if the
    /// pod should be shut down.
    fn should_shutdown(&self, config: &config::AutoShutdown) -> bool {
        let now = chrono::Utc::now();

        let netcfg = config.tcp_idle.as_ref();
        let net_idle = self.network_idle_since.as_ref();

        let mut should_shutdown = false;

        if let Some((cfg, since)) = netcfg.zip(net_idle) {
            let exceeded =
                now.signed_duration_since(*since).to_std().unwrap() > cfg.minimum_idle_time;
            if !exceeded {
                return false;
            } else {
                should_shutdown = true;
            }
        }

        let cpucfg = config.cpu_usage.as_ref();
        let cpu_idle = self.cpu_idle_since.as_ref();

        if let Some((cfg, since)) = cpucfg.zip(cpu_idle) {
            let exceeded =
                now.signed_duration_since(*since).to_std().unwrap() > cfg.minimum_idle_time;
            if !exceeded {
                return false;
            } else {
                should_shutdown = true;
            }
        }

        should_shutdown
    }
}
