use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("ресурс не найден")]
    NotFound,
    #[error("неверные данные запроса")]
    Validation(&'static str),
    #[error("ресурс уже существует")]
    Conflict,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "Ресурс не найден"),
            Self::Validation(message) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
            }
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "Запись с таким адресом уже есть в расследовании",
            ),
            Self::Database(error) => {
                if let sqlx::Error::Database(database_error) = &error
                    && database_error.code().as_deref() == Some("23505")
                {
                    return (
                        StatusCode::CONFLICT,
                        Json(ErrorResponse {
                            error: ErrorBody {
                                code: "conflict",
                                message: "Запись с таким адресом уже есть в расследовании",
                            },
                        }),
                    )
                        .into_response();
                }
                error!(error = %error, "ошибка базы данных");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Не удалось выполнить запрос",
                )
            }
        };
        (
            status,
            Json(ErrorResponse {
                error: ErrorBody { code, message },
            }),
        )
            .into_response()
    }
}
