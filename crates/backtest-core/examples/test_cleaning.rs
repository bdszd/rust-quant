use backtest_core::load_clean_bars;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "data/raw/tushare/000001.SZ/20240101_20241231.csv";

    let result = load_clean_bars(path)?;

    // 输出质量报告
    result.quality.print_summary();

    // 输出清洗后的前五条数据
    println!("\nCleaned bars:");

    for bar in result.bars.iter().take(5) {
        println!("{:?}", bar);
    }

    // 输出异常数据
    println!("\nRejected records:");

    for record in &result.rejected {
        println!(
            "row={}, code={}, date={}, reasons={:?}",
            record.row_number, record.ts_code, record.trade_date, record.reasons
        );
    }

    Ok(())
}
