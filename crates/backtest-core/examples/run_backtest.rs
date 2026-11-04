use backtest_core::{BacktestConfig, ExecutionMode, SmaCrossStrategy, load_bars, run};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/AAPL.csv".to_string());

    let bars = load_bars(&path)?;

    if bars.is_empty() {
        println!("行情文件为空，没有可回测的数据。");
        return Ok(());
    }

    let config = BacktestConfig {
        initial_cash: 1000.0,
        commission_rate: 0.001,
        slippage_rate: 0.001,
        execution_mode: ExecutionMode::NextOpen,
    };

    let mut strategy = SmaCrossStrategy::new(2, 3);
    let result = run(&bars, &mut strategy, &config);

    println!("========== 回测结果 ==========");
    println!("行情文件：{}", path);
    println!("行情记录数：{}", bars.len());
    println!("执行模式：{:?}", config.execution_mode);
    println!("初始资金：{:.3}", result.initial_cash);
    println!("手续费率：{:.3}%", config.commission_rate * 100.0);
    println!("滑点率：{:.3}%", config.slippage_rate * 100.0);
    println!("最终资产：{:.3}", result.final_equity);
    println!("总收益率：{:.3}%", result.total_return * 100.0);
    println!("最大回撤：{:.3}%", result.max_drawdown * 100.0);
    println!("成交笔数：{}", result.trades.len());

    println!("\n========== 交易记录 ==========");

    for trade in &result.trades {
        println!(
            "日期={} 操作={:?} 成交价={:.3} 数量={} 手续费={:.3}",
            trade.date, trade.side, trade.price, trade.quantity, trade.fee,
        );
    }

    println!("\n========== 每日账户资产 ==========");

    for snapshot in &result.history {
        println!(
            "日期={} 收盘价={:.2} 现金={:.3} 持仓={} 总资产={:.3}",
            snapshot.date, snapshot.close, snapshot.cash, snapshot.position, snapshot.equity,
        );
    }

    Ok(())
}
