pub mod attempt;
pub mod job;
pub mod result;
pub mod task;

pub use job::{BacktestJob, BacktestJobConfig, ExecutionMode, StrategyConfig};
pub use task::{BacktestTask, TaskStatus};
