use std::fmt;

/// 回测应用层错误，区分用户输入问题与基础设施故障。
#[derive(Debug)]
pub enum BacktestApplicationError {
    InvalidConfig(String),
    DatasetNotFound(String),
    DatasetCatalog(String),
    Repository(String),
}

impl fmt::Display for BacktestApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message)
            | Self::DatasetCatalog(message)
            | Self::Repository(message) => formatter.write_str(message),
            Self::DatasetNotFound(dataset) => {
                write!(formatter, "找不到清洗后的数据集：{dataset}")
            }
        }
    }
}

impl std::error::Error for BacktestApplicationError {}
