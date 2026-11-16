use axum::{
    Json,
    extract::{FromRequest, Path, State},
    http::{Request, StatusCode},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError};

pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_DETAILS_BYTES: usize = 8 * 1024;
const MAX_SOURCE_REFERENCE_LENGTH: usize = 512;
const MAX_SIGNAL_KEY_LENGTH: usize = 120;
const MAX_SOURCE_NAME_LENGTH: usize = 120;
const MAX_FUTURE_CLOCK_SKEW: Duration = Duration::minutes(5);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalCategory {
    Sanctions,
    Exposure,
    Behavior,
    Other,
}

impl SignalCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sanctions => "sanctions",
            Self::Exposure => "exposure",
            Self::Behavior => "behavior",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSeverity {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

impl SignalSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Informational => "informational",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRiskSignal {
    pub signal_key: String,
    pub category: SignalCategory,
    pub severity: SignalSeverity,
    pub source_name: String,
    pub source_reference: String,
    pub observed_at: DateTime<Utc>,
    pub details: Value,
}

#[derive(Debug)]
struct ValidRiskSignal {
    signal_key: String,
    category: SignalCategory,
    severity: SignalSeverity,
    source_name: String,
    source_reference: String,
    observed_at: DateTime<Utc>,
    details: Value,
}

pub(crate) struct RiskSignalJson<T>(T);

impl<S, T> FromRequest<S> for RiskSignalJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request(
        request: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|rejection| match rejection {
                axum::extract::rejection::JsonRejection::BytesRejection(error)
                    if error.status() == StatusCode::PAYLOAD_TOO_LARGE =>
                {
                    ApiError::PayloadTooLarge
                }
                axum::extract::rejection::JsonRejection::JsonDataError(_) => {
                    ApiError::Validation("Поля JSON не соответствуют формату API")
                }
                _ => ApiError::MalformedJson,
            })?;
        Ok(Self(value))
    }
}

impl TryFrom<SaveRiskSignal> for ValidRiskSignal {
    type Error = ApiError;

    fn try_from(input: SaveRiskSignal) -> Result<Self, Self::Error> {
        let signal_key = required_text(
            input.signal_key,
            MAX_SIGNAL_KEY_LENGTH,
            "Ключ сигнала должен содержать от 1 до 120 символов",
        )?;
        let source_name = required_text(
            input.source_name,
            MAX_SOURCE_NAME_LENGTH,
            "Название источника должно содержать от 1 до 120 символов",
        )?;
        let source_reference = required_text(
            input.source_reference,
            MAX_SOURCE_REFERENCE_LENGTH,
            "Ссылка или ID источника должны содержать от 1 до 512 символов",
        )?;

        if input.observed_at > Utc::now() + MAX_FUTURE_CLOCK_SKEW {
            return Err(ApiError::Validation(
                "Время наблюдения не может быть в будущем",
            ));
        }

        let details_bytes = serde_json::to_vec(&input.details)
            .map_err(|_| ApiError::Validation("Не удалось прочитать детали сигнала"))?;
        if !input.details.is_object()
            || input
                .details
                .as_object()
                .is_some_and(|object| object.is_empty())
        {
            return Err(ApiError::Validation(
                "Детали сигнала должны быть непустым JSON-объектом",
            ));
        }
        if details_bytes.len() > MAX_DETAILS_BYTES {
            return Err(ApiError::Validation("Детали сигнала превышают лимит 8 КиБ"));
        }

        Ok(Self {
            signal_key,
            category: input.category,
            severity: input.severity,
            source_name,
            source_reference,
            observed_at: input.observed_at,
            details: input.details,
        })
    }
}

fn required_text(
    value: String,
    max_chars: usize,
    error_message: &'static str,
) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(ApiError::Validation(error_message));
    }
    Ok(value.to_owned())
}

#[derive(Debug, FromRow, Serialize)]
pub struct RiskSignal {
    pub id: Uuid,
    pub tracked_address_id: Uuid,
    pub signal_key: String,
    pub category: String,
    pub severity: String,
    pub source_name: String,
    pub source_reference: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub details: Value,
    pub created_at: DateTime<Utc>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Path((investigation_id, address_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<RiskSignal>>, ApiError> {
    ensure_scoped_address(&state, investigation_id, address_id).await?;

    let signals = sqlx::query_as::<_, RiskSignal>(
        "SELECT id, tracked_address_id, signal_key, category, severity, source_name, \
         source_reference, observed_at, details, created_at FROM risk_signals \
         WHERE tracked_address_id = $1 ORDER BY observed_at DESC, created_at DESC LIMIT 500",
    )
    .bind(address_id)
    .fetch_all(&state.database)
    .await?;
    Ok(Json(signals))
}

pub(crate) async fn get_one(
    State(state): State<ApiState>,
    Path((investigation_id, address_id, signal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<RiskSignal>, ApiError> {
    let signal = sqlx::query_as::<_, RiskSignal>(
        "SELECT rs.id, rs.tracked_address_id, rs.signal_key, rs.category, rs.severity, \
         rs.source_name, rs.source_reference, rs.observed_at, rs.details, rs.created_at \
         FROM risk_signals rs JOIN tracked_addresses ta ON ta.id = rs.tracked_address_id \
         WHERE rs.id = $3 AND ta.investigation_id = $1 AND ta.id = $2",
    )
    .bind(investigation_id)
    .bind(address_id)
    .bind(signal_id)
    .fetch_optional(&state.database)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(signal))
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Path((investigation_id, address_id)): Path<(Uuid, Uuid)>,
    RiskSignalJson(input): RiskSignalJson<SaveRiskSignal>,
) -> Result<(StatusCode, Json<RiskSignal>), ApiError> {
    let signal = ValidRiskSignal::try_from(input)?;
    let inserted = sqlx::query_as::<_, RiskSignal>(
        "INSERT INTO risk_signals \
         (tracked_address_id, signal_key, category, severity, source_name, source_reference, observed_at, details) \
         SELECT ta.id, $3, $4, $5, $6, $7, $8, $9 FROM tracked_addresses ta \
         WHERE ta.id = $2 AND ta.investigation_id = $1 \
         RETURNING id, tracked_address_id, signal_key, category, severity, source_name, \
         source_reference, observed_at, details, created_at",
    )
        .bind(investigation_id)
        .bind(address_id)
        .bind(signal.signal_key)
        .bind(signal.category.as_str())
        .bind(signal.severity.as_str())
        .bind(signal.source_name)
        .bind(signal.source_reference)
        .bind(signal.observed_at)
        .bind(signal.details)
        .fetch_optional(&state.database)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok((StatusCode::CREATED, Json(inserted)))
}

pub(crate) async fn update(
    State(state): State<ApiState>,
    Path((investigation_id, address_id, signal_id)): Path<(Uuid, Uuid, Uuid)>,
    RiskSignalJson(input): RiskSignalJson<SaveRiskSignal>,
) -> Result<Json<RiskSignal>, ApiError> {
    let signal = ValidRiskSignal::try_from(input)?;
    let updated = sqlx::query_as::<_, RiskSignal>(
        "UPDATE risk_signals rs SET signal_key = $4, category = $5, severity = $6, \
         source_name = $7, source_reference = $8, observed_at = $9, details = $10 \
         FROM tracked_addresses ta WHERE rs.id = $3 AND rs.tracked_address_id = ta.id \
         AND ta.investigation_id = $1 AND ta.id = $2 RETURNING rs.id, rs.tracked_address_id, \
         rs.signal_key, rs.category, rs.severity, rs.source_name, rs.source_reference, \
         rs.observed_at, rs.details, rs.created_at",
    )
    .bind(investigation_id)
    .bind(address_id)
    .bind(signal_id)
    .bind(signal.signal_key)
    .bind(signal.category.as_str())
    .bind(signal.severity.as_str())
    .bind(signal.source_name)
    .bind(signal.source_reference)
    .bind(signal.observed_at)
    .bind(signal.details)
    .fetch_optional(&state.database)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(updated))
}

pub(crate) async fn delete(
    State(state): State<ApiState>,
    Path((investigation_id, address_id, signal_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM risk_signals rs USING tracked_addresses ta \
         WHERE rs.id = $3 AND rs.tracked_address_id = ta.id \
         AND ta.investigation_id = $1 AND ta.id = $2 RETURNING rs.id",
    )
    .bind(investigation_id)
    .bind(address_id)
    .bind(signal_id)
    .fetch_optional(&state.database)
    .await?;
    deleted.ok_or(ApiError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_scoped_address(
    state: &ApiState,
    investigation_id: Uuid,
    address_id: Uuid,
) -> Result<(), ApiError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM tracked_addresses WHERE id = $1 AND investigation_id = $2)",
    )
    .bind(address_id)
    .bind(investigation_id)
    .fetch_one(&state.database)
    .await?;
    exists.then_some(()).ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use chrono::Duration;
    use tower::ServiceExt;

    use super::{
        MAX_DETAILS_BYTES, MAX_REQUEST_BYTES, SaveRiskSignal, SignalCategory, SignalSeverity,
        ValidRiskSignal,
    };
    use crate::{app, health::unavailable_state};

    fn valid_input() -> SaveRiskSignal {
        SaveRiskSignal {
            signal_key: "  indirect_service_exposure  ".into(),
            category: SignalCategory::Exposure,
            severity: SignalSeverity::High,
            source_name: "  Analyst review  ".into(),
            source_reference: "  case-123/source-5  ".into(),
            observed_at: chrono::Utc::now(),
            details: serde_json::json!({"hops": 1, "notes": "Reviewed manually"}),
        }
    }

    #[test]
    fn trims_required_provenance_fields_before_persisting() {
        let signal = ValidRiskSignal::try_from(valid_input()).unwrap();

        assert_eq!(signal.signal_key, "indirect_service_exposure");
        assert_eq!(signal.source_name, "Analyst review");
        assert_eq!(signal.source_reference, "case-123/source-5");
    }

    #[test]
    fn rejects_empty_or_oversized_provenance_fields() {
        let mut input = valid_input();
        input.source_name = "   ".into();
        assert!(ValidRiskSignal::try_from(input).is_err());

        let mut input = valid_input();
        input.source_reference = "x".repeat(513);
        assert!(ValidRiskSignal::try_from(input).is_err());
    }

    #[test]
    fn rejects_invalid_category_and_severity_values() {
        assert!(serde_json::from_str::<SignalCategory>("\"fraud\"").is_err());
        assert!(serde_json::from_str::<SignalSeverity>("\"extreme\"").is_err());
    }

    #[test]
    fn requires_nonempty_object_details_within_the_size_limit() {
        let mut input = valid_input();
        input.details = serde_json::json!({});
        assert!(ValidRiskSignal::try_from(input).is_err());

        let mut input = valid_input();
        input.details = serde_json::json!({"note": "x".repeat(MAX_DETAILS_BYTES)});
        assert!(ValidRiskSignal::try_from(input).is_err());
    }

    #[test]
    fn rejects_observation_times_far_in_the_future() {
        let mut input = valid_input();
        input.observed_at = chrono::Utc::now() + Duration::minutes(6);
        assert!(ValidRiskSignal::try_from(input).is_err());
    }

    #[tokio::test]
    async fn route_rejects_invalid_payload_before_database_access() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/investigations/550e8400-e29b-41d4-a716-446655440000/addresses/550e8400-e29b-41d4-a716-446655440001/risk-signals")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"signal_key":"","category":"exposure","severity":"high","source_name":"source","source_reference":"ref","observed_at":"2026-11-15T10:00:00Z","details":{"note":"test"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let response_body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn route_uses_the_shared_error_envelope_for_invalid_json() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/investigations/550e8400-e29b-41d4-a716-446655440000/addresses/550e8400-e29b-41d4-a716-446655440001/risk-signals")
                    .header("content-type", "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let response_body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(body["error"]["code"], "invalid_json");
    }

    #[tokio::test]
    async fn route_rejects_request_bodies_over_the_limit() {
        let body = format!("{{\"details\":\"{}\"}}", "x".repeat(MAX_REQUEST_BYTES));
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/investigations/550e8400-e29b-41d4-a716-446655440000/addresses/550e8400-e29b-41d4-a716-446655440001/risk-signals")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
        let response_body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(body["error"]["code"], "payload_too_large");
    }
}
