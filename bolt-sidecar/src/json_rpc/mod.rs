use std::convert::Infallible;
use std::sync::Arc;

use api::JsonRpcApi;
use bytes::Bytes;
use secp256k1::SecretKey;
use tokio::sync::mpsc;
use tracing::{error, info};
use warp::{http::Method, reject::Rejection, Filter};

mod api;
mod spec;
mod types;

use self::api::CommitmentsRpc;
use self::spec::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

/// Start the JSON-RPC server. Returns a sender that can be used to send a shutdown signal.
pub async fn start_server(
    port: u16,
    pk: SecretKey,
    relays: Vec<String>,
) -> eyre::Result<mpsc::Sender<()>> {
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    let cors = warp::cors().allow_any_origin().allow_method(Method::POST);

    let rpc_api = api::JsonRpcApi::new(pk, relays);
    let rpc_api_context = Arc::clone(&rpc_api);

    let shutdown_fn = async move {
        shutdown_rx.recv().await;
        rpc_api.shutdown();
    };

    let rpc = warp::post()
        .and(warp::path::end())
        .and(warp::body::bytes())
        .and(warp::header::exact("content-type", "application/json"))
        .and(warp::any().map(move || Arc::clone(&rpc_api_context)))
        .and_then(handle_rpc_request)
        .and_then(|reply| async move { Ok::<_, Rejection>(warp::reply::json(&reply)) })
        .recover(handle_rejection)
        .with(cors);

    let (addr, server) =
        warp::serve(rpc).bind_with_graceful_shutdown(([0, 0, 0, 0], port), shutdown_fn);

    tokio::spawn(server);
    info!("RPC HTTP server listening on http://{}", addr);

    Ok(shutdown_tx)
}

async fn handle_rpc_request(
    req_bytes: Bytes,
    rpc_api: Arc<JsonRpcApi>,
) -> Result<JsonRpcResponse, warp::Rejection> {
    let req = serde_json::from_slice::<JsonRpcRequest>(&req_bytes).map_err(|e| {
        error!(err = ?e, "failed parsing json rpc request");
        warp::reject::custom(JsonRpcError {
            message: "Request parse error".to_string(),
            code: -32700,
            data: None,
        })
    })?;

    tracing::debug!(?req, "received rpc request");

    let res = match req.method.as_str() {
        "bolt_inclusionPreconfirmation" => rpc_api.request_inclusion_commitment(req.params).await?,
        _ => {
            error!(method = ?req.method, "RPC method not found");
            return Err(warp::reject::custom(JsonRpcError {
                message: format!("Method not found: {}", req.method),
                code: -32601,
                data: None,
            }));
        }
    };

    Ok(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: res,
    })
}

async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, Infallible> {
    if let Some(e) = err.find::<JsonRpcError>() {
        Ok(warp::reply::json(e))
    } else if err.is_not_found() {
        Ok(warp::reply::json(&JsonRpcError {
            message: "Resource not found".to_string(),
            code: -32601,
            data: None,
        }))
    } else if let Some(e) = err.find::<warp::reject::MissingHeader>() {
        Ok(warp::reply::json(&JsonRpcError {
            message: format!("Missing header: {}", e.name()),
            code: -32600,
            data: None,
        }))
    } else {
        error!(?err, "unhandled rejection");
        Ok(warp::reply::json(&JsonRpcError {
            message: "Internal error".to_string(),
            code: -32000,
            data: None,
        }))
    }
}
