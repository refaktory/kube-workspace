//! Webserver that serves the API.

mod api;

use axum::{extract::Extension, response::IntoResponse};

use crate::operator::Operator;

/// Start the webserver.
///
/// Blocks indefinitely.
pub async fn run_server(op: Operator) {
    let address = op.config().server_address;

    let router = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/query", axum::routing::post(api_query))
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
                .buffer(512)
                .rate_limit(100, std::time::Duration::from_secs(1))
                .load_shed()
                .timeout(std::time::Duration::from_secs(5))
                .layer(tower_http::trace::TraceLayer::new_for_http())
                .layer(axum::AddExtensionLayer::new(Server { operator: op }))
                .into_inner(),
        );

    tracing::info!(address=%address, "Starting http server");

    axum::Server::bind(&address)
        .serve(router.into_make_service())
        .await
        .unwrap();
}

#[derive(Clone)]
struct Server {
    operator: Operator,
}

type State = axum::extract::Extension<Server>;

async fn health(Extension(_server): State) -> impl IntoResponse {
    (http::StatusCode::OK, "ok".to_string())
}

async fn api_query(
    Extension(server): State,
    query: axum::Json<api::Query>,
) -> axum::Json<api::ApiResult<api::QueryOutput>> {
    let res = api::run_query(&server, &query.0).await;
    tracing::trace!(query=?query, response=?res, "api_query_resolved");
    let output = match res {
        Ok(out) => api::ApiResult::Ok(out),
        Err(err) => api::ApiResult::Error {
            message: err.to_string(),
        },
    };
    axum::Json(output)
}
