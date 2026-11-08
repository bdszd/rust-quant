/// 账户操作失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    InvalidInitialCash,
    InvalidFill(String),
    InvalidAmount,
    InsufficientCash,
    PositionNotFound(String),
    InsufficientSellableQuantity {
        symbol: String,
        requested: u64,
        available: u64,
    },
    QuantityOverflow,
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialCash => {
                write!(f, "初始资金必须是有限的正数")
            }
            Self::InvalidFill(message) => {
                write!(f, "成交记录无效：{message}")
            }
            Self::InvalidAmount => {
                write!(f, "成交金额超出有效数值范围")
            }
            Self::InsufficientCash => {
                write!(f, "账户可用现金不足")
            }
            Self::PositionNotFound(symbol) => {
                write!(f, "{symbol} 没有持仓")
            }
            Self::InsufficientSellableQuantity {
                symbol,
                requested,
                available,
            } => {
                write!(
                    f,
                    "{symbol} 可卖数量不足：请求 {requested}，可卖 {available}"
                )
            }
            Self::QuantityOverflow => {
                write!(f, "持仓数量溢出")
            }
        }
    }
}

impl std::error::Error for AccountError {}