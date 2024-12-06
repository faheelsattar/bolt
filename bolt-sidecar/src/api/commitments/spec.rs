use alloy::primitives::SignatureError;
use axum::{extract::rejection::JsonRejection, http::StatusCode, response::IntoResponse, Json};
use thiserror::Error;

use crate::{
    primitives::{commitment::InclusionCommitment, InclusionRequest},
    state::{consensus::ConsensusError, ValidationError},
};

use super::jsonrpc::JsonResponse;

pub(super) const SIGNATURE_HEADER: &str = "x-bolt-signature";

pub(super) const GET_VERSION_METHOD: &str = "bolt_getVersion";

pub(super) const REQUEST_INCLUSION_METHOD: &str = "bolt_requestInclusion";

pub(super) const GET_METADATA_METHOD: &str = "bolt_metadata";

pub(super) const MAX_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

/// Error type for the commitments API.
#[derive(Debug, Error)]
pub enum CommitmentError {
    /// Request rejected.
    #[error("Request rejected: {0}")]
    Rejected(#[from] RejectionError),
    /// Consensus validation failed.
    #[error("Consensus validation error: {0}")]
    Consensus(#[from] ConsensusError),
    /// Request validation failed.
    #[error("Validation failed: {0}")]
    Validation(#[from] ValidationError),
    /// Duplicate request.
    #[error("Duplicate request")]
    Duplicate,
    /// No available public key to sign commitment request with for a given slot.
    #[error("No available public key to sign request with (slot {0})")]
    NoAvailablePubkeyForSlot(u64),
    /// Internal server error.
    #[error("Internal server error")]
    Internal,
    /// Missing signature.
    #[error("Missing '{SIGNATURE_HEADER}' header")]
    NoSignature,
    /// Invalid signature.
    #[error(transparent)]
    InvalidSignature(#[from] crate::primitives::commitment::SignatureError),
    /// Malformed authentication header.
    #[error("Malformed authentication header")]
    MalformedHeader,
    /// Signature error.
    #[error(transparent)]
    Signature(#[from] SignatureError),
    /// Unknown method.
    #[error("Unknown method")]
    UnknownMethod,
    /// Invalid JSON.
    #[error(transparent)]
    InvalidJson(#[from] JsonRejection),
}

impl IntoResponse for CommitmentError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        match self {
            Self::Rejected(err) => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32000, err.to_string())))
                    .into_response()
            }
            Self::Duplicate => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32001, self.to_string())))
                    .into_response()
            }
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(JsonResponse::from_error(-32002, self.to_string())),
            )
                .into_response(),
            Self::NoAvailablePubkeyForSlot(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(JsonResponse::from_error(-32008, self.to_string())),
            )
                .into_response(),
            Self::NoSignature => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32003, self.to_string())))
                    .into_response()
            }
            Self::InvalidSignature(err) => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32004, err.to_string())))
                    .into_response()
            }
            Self::Signature(err) => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32005, err.to_string())))
                    .into_response()
            }
            Self::Consensus(err) => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32006, err.to_string())))
                    .into_response()
            }
            Self::Validation(err) => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32006, err.to_string())))
                    .into_response()
            }
            Self::MalformedHeader => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32007, self.to_string())))
                    .into_response()
            }
            Self::UnknownMethod => {
                (StatusCode::BAD_REQUEST, Json(JsonResponse::from_error(-32601, self.to_string())))
                    .into_response()
            }
            Self::InvalidJson(err) => (
                StatusCode::BAD_REQUEST,
                Json(JsonResponse::from_error(-32600, format!("Invalid request: {err}"))),
            )
                .into_response(),
        }
    }
}

/// Error indicating the rejection of a commitment request. This should
/// be returned to the user.
#[derive(Debug, Error)]
pub enum RejectionError {
    /// State validation failed for this request.
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Implements the commitments-API: <https://chainbound.github.io/bolt-docs/api/rpc>
#[async_trait::async_trait]
pub trait CommitmentsApi {
    /// Implements: <https://chainbound.github.io/bolt-docs/api/rpc#bolt_requestinclusion>
    async fn request_inclusion(
        &self,
        inclusion_request: InclusionRequest,
    ) -> Result<InclusionCommitment, CommitmentError>;
}
