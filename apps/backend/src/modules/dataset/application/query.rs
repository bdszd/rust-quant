use serde::Serialize;

/// PostgreSQL 中的一个可用数据集。
#[derive(Debug, Clone, Serialize)]
pub struct DatasetSummary {
    /// 数据集 UUID，后续创建回测任务时使用。
    pub id: String,
    /// 数据集显示名称。
    pub name: String,
    /// 该数据集包含的标准日线记录数。
    pub rows: i64,
}
