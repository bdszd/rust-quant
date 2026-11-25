use super::query::DatasetSummary;

/// 数据集目录端口：定义应用层需要的目录查询能力。
pub trait DatasetCatalog: Send + Sync {
    fn exists(&self, dataset: &str) -> Result<bool, String>;

    fn list(&self) -> Result<Vec<DatasetSummary>, String>;
}
