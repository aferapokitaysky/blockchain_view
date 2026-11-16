use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError};

#[derive(Debug, Deserialize)]
pub struct CreateInvestigation {
    pub name: String,
}

#[derive(Debug, FromRow, Serialize)]
pub struct Investigation {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
) -> Result<Json<Vec<Investigation>>, ApiError> {
    let investigations = sqlx::query_as::<_, Investigation>(
        "SELECT id, name, created_at FROM investigations ORDER BY created_at DESC",
    )
    .fetch_all(&state.database)
    .await?;
    Ok(Json(investigations))
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Json(input): Json<CreateInvestigation>,
) -> Result<(StatusCode, Json<Investigation>), ApiError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ApiError::Validation(
            "Название должно содержать от 1 до 120 символов",
        ));
    }

    let investigation = sqlx::query_as::<_, Investigation>(
        "INSERT INTO investigations (name) VALUES ($1) RETURNING id, name, created_at",
    )
    .bind(name)
    .fetch_one(&state.database)
    .await?;

    Ok((StatusCode::CREATED, Json(investigation)))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::{app, health::unavailable_state};

    #[tokio::test]
    async fn rejects_blank_investigation_name_without_database_access() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/investigations")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
