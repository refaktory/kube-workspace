//! Expose prometheus metrics.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::extract::Extension;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use prometheus_client::encoding::text::Encode;

use crate::operator::OperatorMetrics;

/// prometheus-operator ServiceMonitor
/// See https://prometheus-operator.dev/docs/operator/api/#servicemonitorspec
#[derive(kube::CustomResource, Debug, serde::Serialize, serde::Deserialize, Default, Clone)]
#[kube(
    group = "monitoring.coreos.com",
    version = "v1",
    kind = "ServiceMonitor",
    namespaced,
    schema = "disabled"
)]
pub struct ServiceMonitorSpec {
    #[serde(rename = "joinLabel")]
    pub join_label: Option<String>,
    pub selector: LabelSelector,
    pub endpoints: Vec<Endpoint>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Endpoint {
    /// port name
    pub port: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Hash, PartialEq, Eq, Encode)]
struct Labels {}

type Registry = prometheus_client::registry::Registry<
    Box<dyn prometheus_client::encoding::text::SendEncodeMetric>,
>;

fn build_registry(metrics: &OperatorMetrics) -> Registry {
    let mut reg = Registry::default();

    reg.register(
        "kube_workspace_configuration_errors",
        "Number of invalid configurations found. This is either 0 or 1.",
        Box::new(metrics.configuration_errors.clone()),
    );
    reg.register(
        "kube_workspace_available_count",
        "Number of available (active and reachable) workspaces.",
        Box::new(metrics.workspace_available_count.clone()),
    );
    reg.register(
        "kube_workspace_unavailable_count",
        "Number of unavailable (failing) workspaces.",
        Box::new(metrics.workspace_unavailable_count.clone()),
    );

    reg
}

type State = Arc<Mutex<Registry>>;

async fn handler(Extension(registry): Extension<State>) -> impl axum::response::IntoResponse {
    let mut buffer = Vec::new();
    prometheus_client::encoding::text::encode(&mut buffer, &registry.lock().unwrap()).unwrap();

    axum::response::Response::builder()
        .status(http::StatusCode::OK)
        .header(
            http::header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
        )
        .body(axum::body::Body::from(buffer))
        .unwrap()
}

async fn run_exporter(metrics: OperatorMetrics, address: SocketAddr) -> Result<(), anyhow::Error> {
    let registry: State = Arc::new(Mutex::new(build_registry(&metrics)));

    let router = axum::Router::new()
        .route("/metrics", axum::routing::get(handler))
        .layer(
            tower::ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |error: axum::BoxError| async move {
                        if error.is::<tower::timeout::error::Elapsed>() {
                            Result::<(), _>::Err((
                                http::StatusCode::REQUEST_TIMEOUT,
                                "Request has timed out".to_string(),
                            ))
                        } else if error.is::<tower::load_shed::error::Overloaded>() {
                            Err((
                                http::StatusCode::SERVICE_UNAVAILABLE,
                                "API is overloaded".to_string(),
                            ))
                        } else {
                            Err((
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Unhandled internal error: {}", error),
                            ))
                        }
                    },
                ))
                .buffer(5)
                .rate_limit(5, std::time::Duration::from_secs(1))
                .load_shed()
                .layer(tower_http::trace::TraceLayer::new_for_http())
                .layer(axum::AddExtensionLayer::new(registry))
                .into_inner(),
        );

    tracing::info!(address=%address, "starting prometheus metrics exporter");

    axum::Server::bind(&address)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}

pub async fn run_exporter_service(
    metrics: OperatorMetrics,
    address: SocketAddr,
) -> Result<(), anyhow::Error> {
    loop {
        if let Err(err) = tokio::spawn(run_exporter(metrics.clone(), address)).await {
            tracing::error!(?err, "prometheus metrics exporter failed");
            // TODO: exponential backoff?
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    }
}
