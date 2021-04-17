//! Webserver that serves the API.

use crate::operator::Operator;

/// Start the webserver.
///
/// Blocks indefinitely.
pub async fn run_server(op: Operator) {
    let port = op.config().server_port.unwrap_or(8080);
    let routes = filters::routes(op);
    tracing::info!(port = port, "Starting http server");
    warp::serve(routes).run(([0, 0, 0, 0], port)).await
}

mod api {
    //! API handler logic and types.

    use crate::{operator, AnyError};

    use super::Operator;

    pub async fn run_query(op: Operator, query: &Query) -> Result<QueryOutput, AnyError> {
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

                Ok(QueryOutput::PodStart(WorkspaceStatus {
                    phase: status.phase,
                    ssh_address,
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

                Ok(QueryOutput::PodStatus(WorkspaceStatus {
                    phase: status.phase,
                    ssh_address,
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
    pub enum Query {
        PodStart(PodStartRequest),
        PodStatus(PodStatusRequest),
        PodStop(PodStopRequest),
    }

    #[derive(serde::Deserialize, Clone, Debug)]
    pub struct PodStartRequest {
        username: String,
        ssh_public_key: String,
    }

    #[derive(serde::Deserialize, Clone, Debug)]
    pub struct PodStatusRequest {
        username: String,
        ssh_public_key: String,
    }
    #[derive(serde::Deserialize, Clone, Debug)]
    pub struct PodStopRequest {
        username: String,
        ssh_public_key: String,
    }
    #[derive(serde::Serialize, Clone, Debug)]
    pub struct SshAddress {
        pub address: String,
        pub port: i32,
    }

    #[derive(serde::Serialize, Clone, Debug)]
    pub struct WorkspaceStatus {
        phase: operator::WorkspacePhase,
        ssh_address: Option<SshAddress>,
    }

    #[derive(serde::Serialize, Clone, Debug)]
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
}

mod filters {
    //! `warp` filters that define routing and parse/extract queries.

    use warp::{path, Filter};

    use super::{api, handlers, Operator};

    /// Builds all server routes.
    pub fn routes(
        op: Operator,
    ) -> impl warp::Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        api_query(op.clone())
            .or(healthcheck(op))
            .with(warp::log("server"))
    }

    /// Primary API endpoint.
    fn api_query(
        op: Operator,
    ) -> impl warp::Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        path!("api" / "query")
            .and(warp::post())
            .and(json_body::<api::Query>())
            .and(warp::any().map(move || op.clone()))
            .and_then(handlers::api_query)
    }

    fn json_body<T>() -> impl Filter<Extract = (T,), Error = warp::Rejection> + Clone
    where
        T: Send + serde::de::DeserializeOwned,
    {
        // When accepting a body, we want a JSON body
        // (and to reject huge payloads)...
        warp::body::content_length_limit(1024 * 16).and(warp::body::json())
    }

    fn healthcheck(
        _op: Operator,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        path!("health").and(warp::get()).map(|| "ok")
    }
}

mod handlers {
    //! Warp handler functions that take input and produce responses.
    //! These methods contain the actual endpoint logic.

    use super::{api, Operator};
    use std::convert::Infallible;

    pub async fn api_query(
        query: api::Query,
        op: Operator,
    ) -> Result<impl warp::Reply, Infallible> {
        let res = api::run_query(op, &query).await;
        tracing::trace!(query=?query, response=?res, "api_query_resolved");
        let output = match res {
            Ok(out) => api::ApiResult::Ok(out),
            Err(err) => api::ApiResult::Error {
                message: err.to_string(),
            },
        };
        Ok(warp::reply::json(&output))
    }
}
