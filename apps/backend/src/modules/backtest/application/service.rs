use std::sync::Arc;
use uuid::Uuid;

use crate::modules::dataset::application::ports::DatasetCatalog;

use crate::modules::backtest::domain::{
    BacktestJob, BacktestJobConfig, BacktestTask, StrategyConfig, TaskStatus,
};

use super::{
    commands::CreateBacktestCommand,
    error::BacktestApplicationError,
    ports::BacktestRepository,
    queries::BacktestTaskView,
};

/// 组织回测任务用例。
///
/// Handler 和 Worker 都通过应用层完成业务操作；
/// 该服务本身不依赖 Axum、SQLx 或 RabbitMQ。
pub struct BacktestApplicationService {
    repository: Arc<dyn BacktestRepository>,
    datasets: Arc<dyn DatasetCatalog>,
}

impl BacktestApplicationService {
    pub fn new(repository: Arc<dyn BacktestRepository>, datasets: Arc<dyn DatasetCatalog>) -> Self {
        Self {
            repository,
            datasets,
        }
    }

    pub async fn create_task(
        &self,
        command: CreateBacktestCommand,
    ) -> Result<BacktestTask, BacktestApplicationError> {
        validate_config(&command.config).map_err(BacktestApplicationError::InvalidConfig)?;

        let dataset_exists = self
            .datasets
            .exists(&command.config.dataset)
            .map_err(BacktestApplicationError::DatasetCatalog)?;
        if !dataset_exists {
            return Err(BacktestApplicationError::DatasetNotFound(
                command.config.dataset.clone(),
            ));
        }

        let task = BacktestTask {
            id: Uuid::new_v4(),
            user_id: command.user_id,
            status: TaskStatus::Queued,
        };

        let job = BacktestJob {
            id: Uuid::new_v4(),
            task_id: task.id,
            config: command.config,
        };

        self.repository
            .create_task(&task, &job)
            .await
            .map_err(BacktestApplicationError::Repository)?;
        Ok(task)
    }

    pub async fn get_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<BacktestTaskView>, BacktestApplicationError> {
        self.repository
            .find_task(task_id)
            .await
            .map_err(BacktestApplicationError::Repository)
    }

    pub async fn list_tasks(
        &self,
        limit: i64,
    ) -> Result<Vec<BacktestTaskView>, BacktestApplicationError> {
        self.repository
            .list_tasks(limit.clamp(1, 100))
            .await
            .map_err(BacktestApplicationError::Repository)
    }
}

fn validate_config(config: &BacktestJobConfig) -> Result<(), String> {
    validate_dataset_name(&config.dataset)?;

    if !config.initial_cash.is_finite() || config.initial_cash <= 0.0 {
        return Err("初始资金必须是有限的正数".to_string());
    }

    if !config.commission_rate.is_finite() || !(0.0..1.0).contains(&config.commission_rate) {
        return Err("手续费率必须大于等于 0 且小于 1".to_string());
    }

    if !config.slippage_rate.is_finite() || !(0.0..1.0).contains(&config.slippage_rate) {
        return Err("滑点率必须大于等于 0 且小于 1".to_string());
    }

    if config.lot_size == 0 {
        return Err("每手股数必须大于 0".to_string());
    }

    if !config.stamp_duty_rate.is_finite() || !(0.0..1.0).contains(&config.stamp_duty_rate) {
        return Err("印花税率必须大于等于 0 且小于 1".to_string());
    }

    if !config.min_commission.is_finite() || config.min_commission < 0.0 {
        return Err("最低佣金必须是有限的非负数".to_string());
    }

    if let StrategyConfig::SmaCross {
        short_window,
        long_window,
    } = &config.strategy
    {
        if *short_window == 0 {
            return Err("短期均线窗口必须大于 0".to_string());
        }
        if long_window <= short_window {
            return Err("长期均线窗口必须大于短期均线窗口".to_string());
        }
    }

    Ok(())
}

fn validate_dataset_name(dataset: &str) -> Result<(), String> {
    let mut parts = dataset.split('/');
    let symbol = parts.next().unwrap_or_default();
    let date_range = parts.next().unwrap_or_default();

    let valid_component = |value: &str| {
        !value.is_empty()
            && value.trim() == value
            && value != "."
            && value != ".."
            && !value.contains('\\')
            && !value.contains(':')
    };

    if parts.next().is_some() || !valid_component(symbol) || !valid_component(date_range) {
        return Err("dataset 必须使用有效的 股票代码/日期范围 格式".to_string());
    }

    Ok(())
}
