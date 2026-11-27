use async_trait::async_trait;

use super::query::DatasetSummary;

/// 数据集列表查询能力。具体数据从 PostgreSQL 获取。
#[async_trait]
pub trait DatasetCatalog: Send + Sync {
    async fn list(&self) -> Result<Vec<DatasetSummary>, String>;
}

/// 旧 CSV 回测链路暂时仍通过文件系统确认数据集存在。
/// 下一阶段切换 PostgreSQL 行情查询后，再移除此端口。
pub trait DatasetExistence: Send + Sync {
    fn exists(&self, dataset: &str) -> Result<bool, String>;
}
