use serde::Deserialize;
use std::error::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct Bar {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
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
