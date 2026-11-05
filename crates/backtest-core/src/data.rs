use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    #[serde(default, alias = "ts_code")]
    pub symbol: String,

    #[serde(alias = "trade_date")]
    pub date: String,

    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,

    #[serde(default, alias = "vol")]
    pub volume: f64,

    #[serde(default)]
    pub amount: Option<f64>,
}

pub fn load_bars(path: &str) -> Result<Vec<Bar>, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(path)?;
    let mut bars = Vec::new();

    for row in reader.deserialize() {
        let bar: Bar = row?;
        bars.push(bar);
    }

    Ok(bars)
}
