from pathlib import Path

import pandas as pd


PROJECT_ROOT = Path(__file__).resolve().parents[2]

RAW_DIR = PROJECT_ROOT / "data" / "raw"
CLEANED_DIR = PROJECT_ROOT / "data" / "cleaned"

# Tushare daily 数据字段：
# vol 单位为手，amount 单位为千元。
# 输出字段与 crates/backtest-core/src/data.rs 中的 Bar 对应。
OUTPUT_COLUMNS = [
    "symbol",
    "date",
    "open",
    "high",
    "low",
    "close",
    "volume",
    "amount",
]


def clean_file(input_file: Path, output_dir: Path) -> None:
    print(f"Processing: {input_file}")

    df = pd.read_csv(input_file)

    bars = pd.DataFrame({
        "symbol": df["ts_code"],
        "date": pd.to_datetime(
            df["trade_date"].astype(str),
            format="%Y%m%d",
        ).dt.strftime("%Y-%m-%d"),
        "open": df["open"],
        "high": df["high"],
        "low": df["low"],
        "close": df["close"],
        # Tushare vol: 手；Bar.volume: 股。
        "volume": df["vol"] * 100,
        # Tushare amount: 千元；Bar.amount: 元。
        "amount": df["amount"] * 1000 if "amount" in df.columns else None,
    })

    bars = bars[OUTPUT_COLUMNS].sort_values("date").reset_index(drop=True)

    output_dir.mkdir(parents=True, exist_ok=True)
    output_file = output_dir / "bars.csv"
    bars.to_csv(output_file, index=False)

    print(f"Saved {len(bars)} bars: {output_file}")


def main() -> None:
    csv_files = sorted(RAW_DIR.rglob("*.csv"))

    if not csv_files:
        print(f"No CSV files found in: {RAW_DIR}")
        return

    for input_file in csv_files:
        relative_path = input_file.relative_to(RAW_DIR)

        # 保留原始文件的目录和文件名，避免不同日期范围的数据互相覆盖。
        output_dir = CLEANED_DIR / relative_path.parent / input_file.stem
        clean_file(input_file, output_dir)


if __name__ == "__main__":
    main()
