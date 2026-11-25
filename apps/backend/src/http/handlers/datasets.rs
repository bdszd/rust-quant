use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
};

#[derive(Debug, Serialize)]
pub struct DatasetInfo {
    /// 股票代码/日期范围，例如 000001.SZ/20240101_20241231。
    pub name: String,
    /// 从 quality_report.json 的 accepted_rows 读取。
    pub rows: usize,
}

#[derive(Debug, Deserialize)]
struct DatasetMetadata {
    accepted_rows: usize,
}

pub async fn list_datasets(
    State(state): State<AppState>,
) -> Result<Json<Vec<DatasetInfo>>, (StatusCode, Json<ApiError>)> {
    let data_dir = state.data_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        let symbols =
            std::fs::read_dir(&data_dir).map_err(|e| format!("无法读取清洗数据目录：{e}"))?;

        let mut datasets = Vec::new();

        for symbol_entry in symbols {
            let symbol_entry = symbol_entry.map_err(|e| format!("读取股票代码目录失败：{e}"))?;
            let symbol_path = symbol_entry.path();

            if !symbol_path.is_dir() {
                continue;
            }

            let Some(symbol) = symbol_path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            let date_ranges = std::fs::read_dir(&symbol_path)
                .map_err(|e| format!("读取 {symbol} 的日期目录失败：{e}"))?;

            for date_entry in date_ranges {
                let date_entry = date_entry.map_err(|e| format!("读取日期目录失败：{e}"))?;
                let date_path = date_entry.path();

                if !date_path.is_dir() {
                    continue;
                }

                let Some(date_range) = date_path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };

                let bars_path = date_path.join("bars.csv");
                let metadata_path = date_path.join("quality_report.json");

                if !bars_path.is_file() || !metadata_path.is_file() {
                    continue;
                }

                let metadata_content = std::fs::read_to_string(&metadata_path)
                    .map_err(|e| format!("读取数据集元数据失败：{metadata_path:?}: {e}"))?;

                let metadata: DatasetMetadata = serde_json::from_str(&metadata_content)
                    .map_err(|e| format!("解析数据集元数据失败：{metadata_path:?}: {e}"))?;

                datasets.push(DatasetInfo {
                    name: format!("{symbol}/{date_range}"),
                    rows: metadata.accepted_rows,
                });
            }
        }

        datasets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok::<_, String>(datasets)
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("数据集扫描任务异常：{e}"),
        )
    })?
    .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    Ok(Json(result))
}
