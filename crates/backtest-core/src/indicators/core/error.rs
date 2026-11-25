use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq)]
pub enum IndicatorError {
    InvalidPeriod { period: usize },

    InvalidParameter { name: &'static str, value: f64 },

    InvalidInput { index: usize, reason: &'static str },

    NonFiniteOutput { index: usize },
}

impl fmt::Display for IndicatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPeriod { period } => {
                write!(f, "指标计算周期不合法：{period}")
            }

            Self::InvalidParameter { name, value } => {
                write!(f, "指标参数不合法：{name} = {value}")
            }

            Self::InvalidInput { index, reason } => {
                write!(f, "指标输入数据不合法：index={index}, {reason}")
            }

            Self::NonFiniteOutput { index } => {
                write!(f, "指标计算产生非有限数值：index={index}")
            }
        }
    }
}

impl Error for IndicatorError {}
