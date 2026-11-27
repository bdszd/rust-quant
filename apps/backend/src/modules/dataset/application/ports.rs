use async_trait::async_trait;

use super::query::DatasetSummary;

/// 数据集列表查询能力。具体数据从 PostgreSQL 获取。
#[async_trait]
pub trait DatasetCatalog: Send + Sync {
    async fn list(&self) -> Result<Vec<DatasetSummary>, String>;
}
