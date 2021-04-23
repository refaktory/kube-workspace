//! Kubernetes API client wrapper.

use k8s_openapi::api::core::v1::{Namespace, Node, PersistentVolumeClaim, Pod, Service};
use kube::{api::DeleteParams, Api};

/// Kubernetes API client.
/// A convenience wrapper around the API provided by the `kube` crate to make
/// usage easier.
///
/// All Kubernetes API acces goes through this client.
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
