use axum::{Json, extract::State, http::StatusCode};

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
};

pub async fn list_datasets(
    State(state): State<AppState>,
) -> Result<
    Json<Vec<crate::modules::dataset::application::query::DatasetSummary>>,
    (StatusCode, Json<ApiError>),
> {
    let datasets = state
        .datasets
        .list_datasets()
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    Ok(Json(datasets))
}
