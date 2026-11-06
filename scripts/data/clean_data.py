from pathlib import Path
import json

import pandas as pd


PROJECT_ROOT = Path(__file__).resolve().parents[2]

RAW_DIR = PROJECT_ROOT / "data" / "raw"
CLEANED_DIR = PROJECT_ROOT / "data" / "cleaned"

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
    metadata_file = output_dir / "quality_report.json"

    bars.to_csv(output_file, index=False)

    # 当前 Python 清洗脚本负责字段映射、单位转换和排序。
    # accepted_rows 是最终输出的标准行情行数，不在这里虚构质量指标。
    metadata = {
        "total_rows": int(len(df)),
        "accepted_rows": int(len(bars)),
    }
    metadata_file.write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    print(f"Saved {len(bars)} bars: {output_file}")
    print(f"Saved metadata: {metadata_file}")


def main() -> None:
    csv_files = sorted(RAW_DIR.rglob("*.csv"))

    if not csv_files:
        print(f"No CSV files found in: {RAW_DIR}")
        return

    for input_file in csv_files:
        relative_path = input_file.relative_to(RAW_DIR)
        output_dir = CLEANED_DIR / relative_path.parent / input_file.stem
        clean_file(input_file, output_dir)


if __name__ == "__main__":
    main()
