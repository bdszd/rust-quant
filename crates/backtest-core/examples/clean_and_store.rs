use backtest_core::load_bars;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/cleaned/000001.SZ/20240101_20241231/bars.csv".to_string());

    let bars = load_bars(&path)?;

    println!("Loaded {} cleaned bars from {path}", bars.len());

    for bar in bars.iter().take(5) {
        println!("{bar:?}");
    }

    Ok(())
}
