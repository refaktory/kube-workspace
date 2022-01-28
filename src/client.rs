//! Kubernetes API client wrapper.

use anyhow::{anyhow, Context};
use k8s_openapi::{
    api::core::v1::{Namespace, Node, PersistentVolumeClaim, Pod, Service},
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
    apimachinery::pkg::api::resource::Quantity,
    NamespaceResourceScope,
};
use kube::{
    api::{AttachParams, DeleteParams, ListParams, ObjectList, ObjectMeta, Patch, PatchParams},
    Api,
};

use crate::prometheus::ServiceMonitor;
use crate::AnyError;

/// Kubernetes API client.
/// A convenience wrapper around the API provided by the `kube` crate to make
/// usage easier.
///
/// All Kubernetes API access goes through this client.
#[derive(Clone)]
pub struct Client {
    kube: kube::Client,
}

impl Client {
    /// Validate configuration and connect to the API.
    pub async fn connect() -> Result<Self, kube::Error> {
        let kube = kube::Client::try_default().await?;
        Ok(Self { kube })
    }

    /// Convert a `404` (http not found) error result into an `Option<T>`.
    fn api_result_opt<T>(res: Result<T, kube::Error>) -> Result<Option<T>, kube::Error> {
        match res {
            Ok(n) => Ok(Some(n)),
            Err(kube::Error::Api(ref err)) if err.code == 404 => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get a node.
    /// Fails if not found.
    pub async fn node(&self, name: &str) -> Result<Node, kube::Error> {
        Api::<Node>::all(self.kube.clone()).get(name).await
    }

    /// Get a namespace.
    /// Fails if not found.
    pub async fn namespace(&self, name: &str) -> Result<Namespace, kube::Error> {
        Api::<Namespace>::all(self.kube.clone()).get(name).await
    }

    /// Optionally get a namespace.
    pub async fn namespace_opt(&self, name: &str) -> Result<Option<Namespace>, kube::Error> {
        Self::api_result_opt(self.namespace(name).await)
    }

    /// Create a new namespace.
    pub async fn namespace_create(&self, ns: &Namespace) -> Result<Namespace, kube::Error> {
        Api::<Namespace>::all(self.kube.clone())
            .create(&Default::default(), ns)
            .await
    }

    /// Get a `PersistentVolumeClaim`.
    /// Fails if not found.
    pub async fn volume_claim(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<PersistentVolumeClaim, kube::Error> {
        Api::<PersistentVolumeClaim>::namespaced(self.kube.clone(), namespace)
            .get(name)
            .await
    }

    /// Optionally get a `PersistentVolumeClaim`.
    pub async fn volume_claim_opt(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<PersistentVolumeClaim>, kube::Error> {
        Self::api_result_opt(self.volume_claim(namespace, name).await)
    }

    /// Create a new `PersistentVolumeClaim`.
    pub async fn volume_claim_create(
        &self,
        namespace: &str,
        claim: &PersistentVolumeClaim,
    ) -> Result<PersistentVolumeClaim, kube::Error> {
        Api::<PersistentVolumeClaim>::namespaced(self.kube.clone(), namespace)
            .create(&Default::default(), claim)
            .await
    }

    // pub async fn pod_metrics(
    //     &self,
    //     namespace: &str,
    //     pod_name: &str,
    // ) -> Result<PodMetrics, kube::Error> {
    //     Api::<PodMetrics>::namespaced(self.kube.clone(), namespace)
    //         .get(pod_name)
    //         .await
    // }

    /// Paginated pod metrics.
    pub async fn pod_metrics_list(
        &self,
        namespace: &str,
        cursor: Option<String>,
    ) -> Result<ObjectList<PodMetrics>, kube::Error> {
        Api::<PodMetrics>::namespaced(self.kube.clone(), namespace)
            .list(&ListParams {
                limit: Some(500),
                continue_token: cursor,
                ..Default::default()
            })
            .await
    }

    /// All pod metrics for a namespace.
    pub async fn pod_metrics_list_all(
        &self,
        namespace: &str,
    ) -> Result<Vec<PodMetrics>, kube::Error> {
        let mut cursor = None;
        let mut metrics = Vec::new();
        loop {
            let list = self.pod_metrics_list(namespace, cursor).await?;
            metrics.extend(list.items);
            cursor = list.metadata.continue_;
            if cursor.is_none() {
                break;
            }
        }
        Ok(metrics)
    }

    pub async fn prometheus_servicemonitor_opt(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<ServiceMonitor>, kube::Error> {
        let res = Api::<ServiceMonitor>::namespaced(self.kube.clone(), namespace)
            .get(name)
            .await;
        Self::api_result_opt(res)
    }

    pub async fn prometheus_servicemonitor_create(
        &self,
        namespace: &str,
        mon: ServiceMonitor,
    ) -> Result<ServiceMonitor, kube::Error> {
        Api::<ServiceMonitor>::namespaced(self.kube.clone(), namespace)
            .create(&Default::default(), &mon)
            .await
    }

    // pub async fn custom_resource_dynamic_get_by_name_opt(
    //     &self,
    //     api_group: &str,
    //     api_version: &str,
    //     kind: &str,
    //     namespace: &str,
    //     resource_name: &str,
    // ) -> Result<Option<DynamicObject>, kube::Error> {

    //     let gvk = GroupVersionKind::gvk(api_group, api_version, kind);
    //     let air = ApiResource::from_gvk(&gvk);
    //     let api = Api::<DynamicObject>::namespaced_with(self.kube.clone(), namespace, &air);

    //     let res = api.get(resource_name).await;
    //     Self::api_result_opt(res)
    // }

    pub async fn custom_resource_definition_by_name(
        &self,
        name: &str,
    ) -> Result<Option<CustomResourceDefinition>, kube::Error> {
        let res = Api::<CustomResourceDefinition>::all(self.kube.clone())
            .get(name)
            .await;
        Self::api_result_opt(res)
    }

    // Get paginated pods from a namespace.
    // pub async fn pods(
    //     &self,
    //     namespace: &str,
    //     label_selector: Option<(String, String)>,
    //     cursor: Option<String>,
    // ) -> Result<kube::api::ObjectList<Pod>, kube::Error> {
    //     let sel = label_selector.map(|(key, value)| format!("{}={}", key, value));
    //     let params = kube::api::ListParams {
    //         label_selector: sel,
    //         limit: Some(100),
    //         continue_token: cursor,
    //         ..Default::default()
    //     };
    //     Api::<Pod>::namespaced(self.kube.clone(), namespace)
    //         .list(&params)
    //         .await
    // }

    /// Get all pods from a namespace.
    pub async fn pods_all(
        &self,
        namespace: &str,
        label_selector: Option<(String, String)>,
    ) -> Result<Vec<Pod>, kube::Error> {
        let sel = label_selector.map(|(key, value)| format!("{}={}", key, value));
        let mut pods = Vec::new();
        let api = Api::<Pod>::namespaced(self.kube.clone(), namespace);
        let mut params = kube::api::ListParams {
            label_selector: sel.clone(),
            limit: Some(500),
            continue_token: None,
            ..Default::default()
        };

        loop {
            let list = api.list(&params).await?;
            pods.extend(list.items);
            if list.metadata.continue_.is_none() {
                break;
            }
            params.continue_token = list.metadata.continue_;
        }
        Ok(pods)
    }

    /// Get a `Pod`.
    /// Fails if not found.
    pub async fn pod(&self, namespace: &str, name: &str) -> Result<Pod, kube::Error> {
        Api::<Pod>::namespaced(self.kube.clone(), namespace)
            .get(name)
            .await
    }

    /// Optionally get a `Pod`.
    pub async fn pod_opt(&self, namespace: &str, name: &str) -> Result<Option<Pod>, kube::Error> {
        Self::api_result_opt(self.pod(namespace, name).await)
    }

    // pub async fn pods(&self, namespace: &str) -> Result<Vec<Pod>, AnyError> {
    //     let api: Api<Pod> = Api::namespaced(self.kube.clone(), namespace);

    //     let list = api
    //         .list(&ListParams {
    //             ..Default::default()
    //         })
    //         .await?;
    //     Ok(list.items)
    // }

    /// Create a new `Pod`.
    pub async fn pod_create(&self, namespace: &str, pod: &Pod) -> Result<Pod, kube::Error> {
        Api::<Pod>::namespaced(self.kube.clone(), namespace)
            .create(&Default::default(), pod)
            .await
    }

    /// Patch a pod.
    pub async fn pod_patch(
        &self,
        namespace: &str,
        pod_name: &str,
        patch: &Patch<Pod>,
        params: &PatchParams,
    ) -> Result<Pod, kube::Error> {
        Api::<Pod>::namespaced(self.kube.clone(), namespace)
            .patch(pod_name, params, patch)
            .await
    }

    /// Delete a `Pod`.
    pub async fn pod_delete(&self, namespace: &str, name: &str) -> Result<(), kube::Error> {
        Api::<Pod>::namespaced(self.kube.clone(), namespace)
            .delete(
                name,
                &DeleteParams {
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn pod_exec_stdout(
        &self,
        namespace: &str,
        pod: &str,
        container: &str,
        command: Vec<&str>,
    ) -> Result<String, AnyError> {
        use tokio::io::AsyncReadExt;

        let params = AttachParams {
            container: Some(container.to_string()),
            stdout: true,
            ..Default::default()
        };
        let mut proc = Api::<Pod>::namespaced(self.kube.clone(), namespace)
            .exec(pod, command, &params)
            .await?;

        let mut stdout = String::with_capacity(1000);
        proc.stdout()
            .ok_or_else(|| anyhow!("Stout not attached"))?
            .read_to_string(&mut stdout)
            .await
            .context("Could not read stdout")?;

        let status = proc.await.ok_or_else(|| anyhow!("Pod did not terminate"))?;
        if status.status.map(|x| x == "Success").unwrap_or(false) {
            Ok(stdout)
        } else {
            Err(anyhow!("Process did not terminate successfully"))
        }
    }

    /// Get a `Service`.
    /// Fails if not found.
    pub async fn service(&self, namespace: &str, name: &str) -> Result<Service, kube::Error> {
        Api::<Service>::namespaced(self.kube.clone(), namespace)
            .get(name)
            .await
    }

    /// Optionally get a `Service`.
    pub async fn service_opt(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Service>, kube::Error> {
        Self::api_result_opt(self.service(namespace, name).await)
    }

    /// Create a new `Service`.
    pub async fn service_create(
        &self,
        namespace: &str,
        service: &Service,
    ) -> Result<Service, kube::Error> {
        Api::<Service>::namespaced(self.kube.clone(), namespace)
            .create(&Default::default(), service)
            .await
    }

    /// Delete a `Service`.
    pub async fn service_delete(&self, namespace: &str, name: &str) -> Result<(), kube::Error> {
        Api::<Service>::namespaced(self.kube.clone(), namespace)
            .delete(
                name,
                &DeleteParams {
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodMetricsContainerUsage {
    pub cpu: Quantity,
    pub memory: Quantity,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodMetricsContainer {
    pub name: String,
    pub usage: PodMetricsContainerUsage,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct PodMetrics {
    pub metadata: ObjectMeta,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub window: String,
    pub containers: Vec<PodMetricsContainer>,
}

impl k8s_openapi::Resource for PodMetrics {
    const GROUP: &'static str = "metrics.k8s.io";
    const KIND: &'static str = "pod";
    const VERSION: &'static str = "v1beta1";
    const API_VERSION: &'static str = "metrics.k8s.io/v1beta1";
    const URL_PATH_SEGMENT: &'static str = "pods";

    type Scope = NamespaceResourceScope;
}

impl k8s_openapi::Metadata for PodMetrics {
    type Ty = ObjectMeta;

    fn metadata(&self) -> &Self::Ty {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut Self::Ty {
        &mut self.metadata
    }
}

/// Parse a Kubernetes API quantity into a i64 representation.
fn parse_quantity(q: &Quantity) -> Result<i64, AnyError> {
    let mut number_end_index = 0;
    let mut chars = q.0.chars().peekable();

    match chars.next() {
        None => {
            return Err(anyhow!("Empty quantity"));
        }
        Some(x) => {
            if x.is_ascii_digit() || x == '+' || x == '-' {
                number_end_index += 1;
            } else {
                return Err(anyhow!("Invalid quantity"));
            }
        }
    }
    while chars.peek().map(|x| x.is_ascii_digit()).unwrap_or(false) {
        number_end_index += 1;
        chars.next();
    }

    let number: i64 = q.0[0..number_end_index].parse()?;
    let suffix = &q.0[number_end_index..];
    let mul: f64 = match suffix {
        "m" => 0.001,
        "" => 1.0,
        "k" => 1_000.0,
        "Ki" => 1_024.0,
        "M" => 1_000_000.0,
        "Mi" => 2.0f64.powi(20),
        "G" => 1_000_000_000.0,
        "Gi" => 2.0f64.powi(30),
        "T" => 1_000_000_000_000.0,
        "Ti" => 2.0f64.powi(40),
        "P" => 1_000_000_000_000_000.0,
        "Pi" => 2.0f64.powi(50),
        "E" => 1_000_000_000_000_000_000.0,
        "Ei" => 2.0f64.powi(60),
        other => return Err(anyhow!("Unknown suffix {}", other)),
    };

    Ok((number as f64 * mul).ceil() as i64)
}

/// Get total pod CPU usage for all containers an a pod.
pub fn pod_metrics_total_cpu(metrics: &PodMetrics) -> Result<i64, AnyError> {
    metrics.containers.iter().try_fold(0i64, |acc, container| {
        parse_quantity(&container.usage.cpu).map(|x| x + acc)
    })
}

pub fn pod_name(pod: &Pod) -> &String {
    pod.metadata.name.as_ref().unwrap()
}

/// Extract the NodePort of a `Service`.
pub fn service_get_nodeport(svc: &Service) -> Option<i32> {
    svc.spec.as_ref()?.ports.as_ref()?.first()?.node_port
}

/// Determine if all containers of a `Pod` are ready.
/// Ready means that they are up and running and are passing the readinessCheck
/// if one is configured.
pub fn pod_containers_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|x| x.container_statuses.as_ref())
        .map(|s| s.iter().all(|x| x.ready))
        .unwrap_or_default()
}

/// Get the ip of a Node.
pub fn node_ip(node: &Node) -> Option<String> {
    node.status
        .as_ref()?
        .addresses
        .as_ref()?
        .iter()
        .find(|addr| addr.type_ == "InternalIP")
        .map(|addr| addr.address.clone())
}

#[cfg(test)]
mod tests {
    // use k8s_openapi::api::core::v1::{Container, PodSpec};

    // use super::*;
    // use crate::operator::WorkspacePhase;

    // #[tokio::test]
    // async fn test_metrics() {
    //     let client = Client::connect().await.unwrap();
    //     let _metrics = client.pod_metrics("default", "forever").await.unwrap();
    //     let _list = Api::<PodMetrics>::all(client.kube.clone())
    //         .list(&Default::default())
    //         .await
    //         .unwrap();
    // }

    // #[tokio::test]
    // async fn test_kube_exec() {
    //     let c = Client::connect().await.unwrap();

    //     // Ensure pod does not exist.
    //     c.pod_delete("default", "exec-test").await.ok();
    //     loop {
    //         let pod = c.pod_opt("default", "exec-test").await.unwrap();
    //         if pod.is_none() {
    //             break;
    //         }
    //         eprintln!("Waiting for old pod to shut down");
    //         tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    //     }

    //     eprintln!("Creating pod");
    //     c.pod_create(
    //         "default",
    //         &Pod {
    //             metadata: ObjectMeta {
    //                 name: Some("exec-test".into()),
    //                 namespace: Some("default".into()),
    //                 ..Default::default()
    //             },
    //             spec: Some(PodSpec {
    //                 containers: vec![Container {
    //                     name: "main".to_string(),
    //                     image: Some("debian".to_string()),
    //                     command: Some(vec![
    //                         "sh".to_string(),
    //                         "-c".to_string(),
    //                         "sleep infinity".to_string(),
    //                     ]),
    //                     ..Default::default()
    //                 }],
    //                 ..Default::default()
    //             }),
    //             status: None,
    //         },
    //     )
    //     .await
    //     .unwrap();

    //     // wait until pod is ready.
    //     loop {
    //         let pod = c.pod_opt("default", "exec-test").await.unwrap();
    //         if let Some(pod) = pod {
    //             let phase = WorkspacePhase::from_pod(&pod);
    //             if phase == WorkspacePhase::Ready {
    //                 break;
    //             }
    //         }
    //         tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    //         eprintln!("Waiting for pod to become ready");
    //     }

    //     let stdout = c
    //         .pod_exec_stdout("default", "exec-test", "main", vec!["ls", "/"])
    //         .await
    //         .unwrap();
    //     assert!(stdout.contains("tmp\n"));

    //     c.pod_delete("default", "exec-test").await.unwrap();
    // }
}
