use super::super::domain::BacktestJobConfig;
use uuid::Uuid;

/// 创建回测任务的应用层命令。
#[derive(Debug, Clone)]
pub struct CreateBacktestCommand {
    pub user_id: Option<Uuid>,
    pub config: BacktestJobConfig,
}
