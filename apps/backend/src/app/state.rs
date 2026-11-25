use crate::{
    modules::{
        backtest::application::BacktestApplicationService,
        dataset::application::DatasetApplicationService,
    },
    observability::metrics::Metrics,
};
use identity::auth::{AuthenticationService, RegistrationService};
use sqlx::PgPool;
use std::sync::Arc;

/// API 与后台 Worker 共享的应用状态。
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub metrics: Arc<Metrics>,
    pub backtest: Arc<BacktestApplicationService>,
    pub datasets: Arc<DatasetApplicationService>,

    /// 负责登录、Token 验证和退出登录。
    pub authentication: AuthenticationService,
    pub registration: RegistrationService,
}
