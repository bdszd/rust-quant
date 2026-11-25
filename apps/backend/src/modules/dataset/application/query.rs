use serde::Serialize;

/// 清洗后的数据集目录项。
#[derive(Debug, Clone, Serialize)]
pub struct DatasetSummary {
    /// 股票代码/日期范围，例如 000001.SZ/20240101_20241231。
    pub name: String,
    /// 从 quality_report.json 的 accepted_rows 读取。
    pub rows: usize,
}
