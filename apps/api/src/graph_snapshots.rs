use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError};

pub const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_GRAPH_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_TRANSFERS: usize = 10_000;

#[derive(Debug, Deserialize)]
pub struct CreateGraphSnapshot {
    title: String,
    focus_network: Option<String>,
    focus_address: Option<String>,
    max_depth: i16,
    source_transfer_ids: Vec<Uuid>,
    #[serde(default)]
    source_relationship_ids: Vec<Uuid>,
    graph: Value,
}

#[derive(Debug, FromRow, Serialize)]
pub struct GraphSnapshot {
    id: Uuid,
    investigation_id: Uuid,
    title: String,
    focus_network: Option<String>,
    focus_address: Option<String>,
    max_depth: i16,
    source_transfer_ids: Vec<Uuid>,
    source_relationship_ids: Vec<Uuid>,
    graph: Value,
    created_at: DateTime<Utc>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Path(investigation_id): Path<Uuid>,
) -> Result<Json<Vec<GraphSnapshot>>, ApiError> {
    ensure_investigation_exists(&state, investigation_id).await?;
    let snapshots = sqlx::query_as::<_, GraphSnapshot>(
        "SELECT id, investigation_id, title, focus_network, focus_address, max_depth, \
         source_transfer_ids, source_relationship_ids, graph, created_at FROM investigation_graph_snapshots \
         WHERE investigation_id = $1 ORDER BY created_at DESC LIMIT 100",
    )
    .bind(investigation_id)
    .fetch_all(&state.database)
    .await?;
    Ok(Json(snapshots))
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Path(investigation_id): Path<Uuid>,
    Json(input): Json<CreateGraphSnapshot>,
) -> Result<(StatusCode, Json<GraphSnapshot>), ApiError> {
    validate(&input)?;
    ensure_investigation_exists(&state, investigation_id).await?;

    let valid_transfer_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(DISTINCT transfer.id) FROM observed_transfers AS transfer \
         JOIN tracked_addresses AS address ON address.id = transfer.tracked_address_id \
         WHERE address.investigation_id = $1 AND transfer.id = ANY($2)",
    )
    .bind(investigation_id)
    .bind(&input.source_transfer_ids)
    .fetch_one(&state.database)
    .await?;
    let valid_relationship_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(DISTINCT relationship.id) FROM observed_relationships AS relationship \
         JOIN tracked_addresses AS address ON address.id = relationship.tracked_address_id \
         WHERE address.investigation_id = $1 AND relationship.id = ANY($2)",
    )
    .bind(investigation_id)
    .bind(&input.source_relationship_ids)
    .fetch_one(&state.database)
    .await?;
    if valid_transfer_count != input.source_transfer_ids.len() as i64
        || valid_relationship_count != input.source_relationship_ids.len() as i64
    {
        return Err(ApiError::Validation(
            "Снимок содержит наблюдения не из этого расследования",
        ));
    }

    let title = input.title.trim();
    let snapshot = sqlx::query_as::<_, GraphSnapshot>(
        "INSERT INTO investigation_graph_snapshots \
         (investigation_id, title, focus_network, focus_address, max_depth, source_transfer_ids, source_relationship_ids, graph) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, investigation_id, title, focus_network, focus_address, max_depth, \
         source_transfer_ids, source_relationship_ids, graph, created_at",
    )
    .bind(investigation_id)
    .bind(title)
    .bind(input.focus_network.as_deref())
    .bind(input.focus_address.as_deref())
    .bind(input.max_depth)
    .bind(&input.source_transfer_ids)
    .bind(&input.source_relationship_ids)
    .bind(input.graph)
    .fetch_one(&state.database)
    .await?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

async fn ensure_investigation_exists(
    state: &ApiState,
    investigation_id: Uuid,
) -> Result<(), ApiError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM investigations WHERE id = $1)")
            .bind(investigation_id)
            .fetch_one(&state.database)
            .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

fn validate(input: &CreateGraphSnapshot) -> Result<(), ApiError> {
    let title_length = input.title.trim().chars().count();
    if title_length == 0 || title_length > 120 {
        return Err(ApiError::Validation(
            "Название снимка должно содержать от 1 до 120 символов",
        ));
    }
    if !(1..=3).contains(&input.max_depth) {
        return Err(ApiError::Validation("Глубина графа должна быть от 1 до 3"));
    }
    if input.source_transfer_ids.len() + input.source_relationship_ids.len() == 0
        || input.source_transfer_ids.len() + input.source_relationship_ids.len()
            > MAX_SOURCE_TRANSFERS
    {
        return Err(ApiError::Validation(
            "Снимок должен содержать от 1 до 10000 наблюдений",
        ));
    }
    let unique_ids = input
        .source_transfer_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if unique_ids.len() != input.source_transfer_ids.len() {
        return Err(ApiError::Validation(
            "Снимок содержит повторяющиеся наблюдения",
        ));
    }
    let unique_relationship_ids = input
        .source_relationship_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if unique_relationship_ids.len() != input.source_relationship_ids.len() {
        return Err(ApiError::Validation("Снимок содержит повторяющиеся связи"));
    }
    if input.focus_network.is_some() != input.focus_address.is_some()
        || input
            .focus_network
            .as_ref()
            .is_some_and(|network| network.trim().is_empty() || network.len() > 64)
        || input
            .focus_address
            .as_ref()
            .is_some_and(|address| address.trim().is_empty() || address.len() > 128)
    {
        return Err(ApiError::Validation("Адрес фокуса графа указан неверно"));
    }
    if !input.graph.is_object()
        || serde_json::to_vec(&input.graph).map_or(true, |graph| graph.len() > MAX_GRAPH_BYTES)
    {
        return Err(ApiError::Validation(
            "Данные графа пусты или слишком велики",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CreateGraphSnapshot, validate};
    use serde_json::json;
    use uuid::Uuid;

    fn valid_input() -> CreateGraphSnapshot {
        CreateGraphSnapshot {
            title: " Snapshot  ".into(),
            focus_network: Some("tron".into()),
            focus_address: Some("TAddress".into()),
            max_depth: 2,
            source_transfer_ids: vec![Uuid::new_v4()],
            source_relationship_ids: vec![],
            graph: json!({"nodes": [], "edges": []}),
        }
    }

    #[test]
    fn accepts_bounded_graph_snapshot() {
        assert!(validate(&valid_input()).is_ok());
    }

    #[test]
    fn rejects_repeated_transfer_ids_and_invalid_depth() {
        let mut input = valid_input();
        input.source_transfer_ids.push(input.source_transfer_ids[0]);
        assert!(validate(&input).is_err());

        let mut input = valid_input();
        input.max_depth = 4;
        assert!(validate(&input).is_err());
    }

    #[test]
    fn accepts_transfer_only_relationship_only_and_combined_evidence() {
        let mut transfer_only = valid_input();
        assert!(validate(&transfer_only).is_ok());

        transfer_only.source_transfer_ids.clear();
        transfer_only.source_relationship_ids.push(Uuid::new_v4());
        assert!(validate(&transfer_only).is_ok());

        transfer_only.source_transfer_ids.push(Uuid::new_v4());
        assert!(validate(&transfer_only).is_ok());

        transfer_only.source_transfer_ids.clear();
        transfer_only.source_relationship_ids.clear();
        assert!(validate(&transfer_only).is_err());
    }
}
