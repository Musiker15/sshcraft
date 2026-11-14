//! Error answers: always JSON `{"code": "...", "detail": "..."}`, never prose for people.

use crate::store::StoreError;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::json;
use sshcraft_proto::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("bad api key")]
    BadApiKey,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("account locked")]
    AccountLocked,
    #[error("device inactive")]
    DeviceInactive,
    #[error("device blocked")]
    DeviceBlocked,
    #[error("not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("conflict, current {current}")]
    Conflict { current: u64 },
    #[error("email taken")]
    EmailTaken,
    #[error("account exists")]
    AccountExists,
    #[error("device id taken")]
    DeviceIdTaken,
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("too large")]
    TooLarge,
    #[error("busy")]
    Busy,
    #[error("internal error")]
    Internal,
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadApiKey => "bad_api_key",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::AccountLocked => "account_locked",
            Self::DeviceInactive => "device_inactive",
            Self::DeviceBlocked => "device_blocked",
            Self::NotFound => "not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::Conflict { .. } => "conflict",
            Self::EmailTaken => "email_taken",
            Self::AccountExists => "account_exists",
            Self::DeviceIdTaken => "device_id_taken",
            Self::Invalid(_) => "invalid",
            Self::TooLarge => "too_large",
            Self::Busy => "busy",
            Self::Internal => "internal",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadApiKey | Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::AccountLocked | Self::DeviceInactive | Self::DeviceBlocked => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Conflict { .. } | Self::EmailTaken | Self::AccountExists | Self::DeviceIdTaken => StatusCode::CONFLICT,
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Busy => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Logs the cause and answers `internal`; the cause never reaches the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {cause}");
        Self::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "code": self.code() });
        match &self {
            Self::Conflict { current } => body["current"] = json!(current),
            Self::Invalid(detail) => body["detail"] = json!(detail),
            _ => {}
        }
        (self.status(), axum::Json(body)).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::Conflict { current } => Self::Conflict { current },
            StoreError::EmailTaken => Self::EmailTaken,
            StoreError::AccountExists => Self::AccountExists,
            StoreError::DeviceIdTaken => Self::DeviceIdTaken,
            StoreError::NotFound => Self::NotFound,
            StoreError::NotAllowed => Self::Forbidden,
            StoreError::WrongKeyVersion { current } => Self::Invalid(format!("the area is at key version {current}")),
            StoreError::Backend(cause) => Self::internal(cause),
        }
    }
}

impl From<ProtoError> for ApiError {
    fn from(err: ProtoError) -> Self {
        Self::Invalid(err.to_string())
    }
}

/// `axum::Json` whose rejections answer in the error format.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE { ApiError::TooLarge } else { ApiError::Invalid(rejection.body_text()) }
}

/// `axum::extract::Path` whose rejections answer `invalid`.
pub struct ApiPath<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiPath<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Self(value))
            .map_err(|rejection: PathRejection| ApiError::Invalid(rejection.body_text()))
    }
}

/// `axum::extract::Query` whose rejections answer `invalid`.
pub struct ApiQuery<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Self(value))
            .map_err(|rejection: QueryRejection| ApiError::Invalid(rejection.body_text()))
    }
}
