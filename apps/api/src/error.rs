use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("ресурс не найден")]
    NotFound,
    #[error("неверные данные запроса")]
    Validation(&'static str),
    #[error("некорректный JSON запроса")]
    MalformedJson,
    #[error("размер запроса превышает допустимый лимит")]
    PayloadTooLarge,
    #[error("ресурс уже существует")]
    Conflict,
    #[error("сеть пока не поддерживается синхронизацией")]
    UnsupportedNetwork,
    #[error("провайдер временно недоступен")]
    ProviderUnavailable,
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
            Self::MalformedJson => (
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "Некорректный JSON тела запроса",
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Тело запроса превышает допустимый размер",
            ),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "Запись с таким адресом уже есть в расследовании",
            ),
            Self::UnsupportedNetwork => (
                StatusCode::NOT_IMPLEMENTED,
                "unsupported_network",
                "Синхронизация этой сети пока не поддерживается",
            ),
            Self::ProviderUnavailable => (
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "Источник данных временно недоступен или вернул неверный ответ",
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
