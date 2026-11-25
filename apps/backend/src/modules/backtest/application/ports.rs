use async_trait::async_trait;
use uuid::Uuid;

use super::queries::BacktestTaskView;
use crate::modules::backtest::domain::{BacktestJob, BacktestTask};

/// 应用层需要的持久化能力。
///
/// PostgreSQL 是它的一种实现；应用层不依赖 SQLx。
#[async_trait]
pub trait BacktestRepository: Send + Sync {
    async fn create_task(
        &self,
        task: &BacktestTask,
        job: &BacktestJob,
    ) -> Result<(), String>;

    async fn find_task(&self, task_id: Uuid) -> Result<Option<BacktestTaskView>, String>;

    async fn list_tasks(&self, limit: i64) -> Result<Vec<BacktestTaskView>, String>;
}

/// 应用层需要的数据集目录能力。
///
/// 具体实现负责在文件系统中检查清洗后的数据集是否存在。
pub trait DatasetCatalog: Send + Sync {
    fn exists(&self, dataset: &str) -> Result<bool, String>;
}
