use sqlx::PgPool;
use std::path::PathBuf;

/// API 与后台 Worker 共享的应用状态。
#[derive(Clone)]
pub struct AppState {
    pub data_dir: PathBuf,
    pub db: PgPool,
}
