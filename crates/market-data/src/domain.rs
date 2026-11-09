use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into().trim().to_ascii_uppercase();
        if value.is_empty() || value.len() > 20
            || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        {
            return Err(format!("非法股票代码：{value}"));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceUnit { Cny }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeUnit { Lots, Shares }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountUnit { ThousandCny, Cny }

#[derive(Debug, Clone)]
pub struct RawDailyBar {
    pub symbol: String,
    pub trade_date: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub amount: Option<Decimal>,
    pub price_unit: PriceUnit,
    pub volume_unit: VolumeUnit,
    pub amount_unit: Option<AmountUnit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyBar {
    pub symbol: Symbol,
    pub trade_date: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct IngestionRequest {
    pub symbols: Vec<Symbol>,
    pub start_date: String,
    pub end_date: String,
}
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub symbols: Vec<Symbol>,
    pub start_date: String,
    pub end_date: String,
}
#[derive(Debug, Clone, Default)]
pub struct IngestionResult {
    pub received: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub upserted: usize,
}
