use crate::{
    modules::backtest::application::BacktestApplicationService,
    observability::metrics::Metrics,
};
use identity::auth::AuthenticationService;
use sqlx::PgPool;
use std::{path::PathBuf, sync::Arc};

/// API 与后台 Worker 共享的应用状态。
#[derive(Clone)]
pub struct AppState {
    pub data_dir: PathBuf,
    pub db: PgPool,
    pub metrics: Arc<Metrics>,
    pub backtest: Arc<BacktestApplicationService>,

    /// 负责登录、Token 验证和退出登录。
    pub authentication: AuthenticationService,
}
