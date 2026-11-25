use std::{path::PathBuf, sync::Arc};

use uuid::Uuid;

use crate::{
    modules::backtest::{
        application::{ports::BacktestExecutor, result::BacktestRunResult},
        domain::BacktestJobConfig,
        service::execute_config,
    },
    observability::metrics::Metrics,
};

/// Backtest Core 的具体执行适配器。
///
/// 把运行环境中的数据目录和性能指标注入回测执行过程，
/// 避免 Worker 直接依赖这些技术细节。
pub struct BacktestCoreExecutor {
    data_dir: PathBuf,
    metrics: Arc<Metrics>,
}

impl BacktestCoreExecutor {
    pub fn new(data_dir: PathBuf, metrics: Arc<Metrics>) -> Self {
        Self { data_dir, metrics }
    }
}

impl BacktestExecutor for BacktestCoreExecutor {
    fn execute(
        &self,
        task_id: Uuid,
        config: BacktestJobConfig,
    ) -> Result<BacktestRunResult, String> {
        execute_config(task_id, config, &self.data_dir, self.metrics.clone())
    }
}
