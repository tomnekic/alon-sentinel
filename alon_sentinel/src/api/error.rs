use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

const INTERNAL_SERVER_ERROR_MESSAGE: &str = "internal server error";

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(crate) fn too_many_requests(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
        }
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub(crate) fn unauthorized_error(error: anyhow::Error) -> Self {
        Self::unauthorized(error.to_string())
    }

    pub(crate) fn internal_error(error: anyhow::Error) -> Self {
        tracing::error!(error = ?error, "Internal API error");
        Self::internal(INTERNAL_SERVER_ERROR_MESSAGE)
    }

    pub(crate) fn bad_request_error(error: anyhow::Error) -> Self {
        Self::bad_request(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[cfg(test)]
mod tests {
    use super::ApiError;
    use axum::{body::to_bytes, response::IntoResponse};
    use serde_json::Value;

    #[tokio::test]
    async fn internal_error_hides_underlying_error_message() {
        let response = ApiError::internal_error(anyhow::anyhow!(
            "duplicate key value violates unique constraint sites_base_url_key"
        ))
        .into_response();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should serialize");
        let payload: Value =
            serde_json::from_slice(&body).expect("error response body should be valid JSON");

        assert_eq!(payload["error"], "internal server error");
    }

    #[tokio::test]
    async fn bad_request_error_preserves_client_safe_message() {
        let response =
            ApiError::bad_request_error(anyhow::anyhow!("target_url is not a valid URL"))
                .into_response();

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should serialize");
        let payload: Value =
            serde_json::from_slice(&body).expect("error response body should be valid JSON");

        assert_eq!(payload["error"], "target_url is not a valid URL");
    }
}
