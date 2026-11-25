use backtest_core::ExecutionMode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
/// 注意：这里不保存 data_dir 等机器本地路径。数据目录属于运行环境配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobConfig {
    pub dataset: String,
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
