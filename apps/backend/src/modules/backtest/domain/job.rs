use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 回测执行时点。由 Backend 领域定义，再由执行适配器转换为引擎配置。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    NextOpen,
    NextClose,
}

/// 策略配置是执行参数的一部分，不依赖具体策略实现。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    SmaCross {
        short_window: usize,
        long_window: usize,
    },
    BuyAndHold,
}

/// 一次回测的不可变执行配置。
///
/// 行情选择条件随任务一起持久化，Worker 重试时仍会查询相同的数据集、标的和日期范围。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobConfig {
    /// PostgreSQL market_datasets.id (UUID string).
    pub dataset: String,
    pub symbols: Vec<String>,
    pub start_date: String,
    pub end_date: String,
    pub initial_cash: f64,
    pub commission_rate: f64,
    pub slippage_rate: f64,
    pub lot_size: u64,
    pub stamp_duty_rate: f64,
    pub min_commission: f64,
    pub execution_mode: ExecutionMode,
    pub strategy: StrategyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: Uuid,
    pub task_id: Uuid,
    pub config: BacktestJobConfig,
}
