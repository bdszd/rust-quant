use backtest_core::{
    load_clean_bars, load_cleaned_bars, load_quality_report, save_cleaning_result,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 原始 Tushare CSV
    let raw_path = "data/raw/tushare/000001.SZ/20240101_20241231.csv";

    // 2. 清洗结果目录
    let output_dir = "data/cleaned/000001.SZ/20240101_20241231";

    // 3. 读取并清洗原始数据
    let result = load_clean_bars(raw_path)?;

    println!("Cleaning completed.");

    // 4. 输出质量报告
    result.quality.print_summary();

    // 5. 保存清洗结果
    let paths = save_cleaning_result(&result, output_dir)?;

    println!("\nSaved files:");
    println!("Bars: {:?}", paths.bars_path);
    println!("Rejected: {:?}", paths.rejected_path);
    println!("Quality: {:?}", paths.quality_report_path);

    // 6. 重新读取清洗后的行情
    let bars = load_cleaned_bars(&paths.bars_path)?;

    println!("\nReloaded bars: {}", bars.len());

    for bar in bars.iter().take(5) {
        println!("{:?}", bar);
    }

    // 7. 重新读取质量报告
    let report = load_quality_report(&paths.quality_report_path)?;

    println!("\nLoaded quality report:");
    report.print_summary();

    Ok(())
}
