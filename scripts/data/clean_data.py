import argparse
import json
from datetime import datetime, timezone
from pathlib import Path

import pandas as pd


PROJECT_ROOT = Path(__file__).resolve().parents[2]

RAW_DIR = PROJECT_ROOT / "data" / "raw"
CLEANED_DIR = PROJECT_ROOT / "data" / "cleaned"

REQUIRED_COLUMNS = [
    "ts_code",
    "trade_date",
    "open",
    "high",
    "low",
    "close",
    "vol",
]

NUMERIC_COLUMNS = [
    "open",
    "high",
    "low",
    "close",
    "vol",
]

SCRIPT_VERSION = "0.1.0"


def validate_data(df: pd.DataFrame):
    """
    检查行情数据，返回：
    1. 合格数据
    2. 不合格数据
    3. 质量统计
    """

    df = df.copy()

    original_count = len(df)

    # 记录每一行被剔除的原因
    reject_reasons = pd.Series("", index=df.index, dtype="object")

    def reject(mask, reason):
        """记录不合格行及原因。"""
        mask = mask.fillna(True)
        new_rejects = mask & (reject_reasons == "")
        reject_reasons.loc[new_rejects] = reason

    # 1. 检查必要字段
    missing_columns = [
        column
        for column in REQUIRED_COLUMNS
        if column not in df.columns
    ]

    if missing_columns:
        raise ValueError(
            f"CSV 缺少必要字段: {missing_columns}"
        )

    # 2. 检查股票代码
    reject(
        df["ts_code"].isna()
        | (df["ts_code"].astype(str).str.strip() == ""),
        "missing_ts_code",
    )

    # 3. 检查交易日期
    df["trade_date"] = (
        df["trade_date"]
        .astype(str)
        .str.strip()
    )

    parsed_dates = pd.to_datetime(
        df["trade_date"],
        format="%Y%m%d",
        errors="coerce",
    )

    reject(
        parsed_dates.isna(),
        "invalid_trade_date",
    )

    # 4. 检查数值字段
    for column in NUMERIC_COLUMNS:
        df[column] = pd.to_numeric(
            df[column],
            errors="coerce",
        )

        reject(
            df[column].isna(),
            f"invalid_{column}",
        )

    # 5. 检查价格必须大于 0
    for column in ["open", "high", "low", "close"]:
        reject(
            df[column] <= 0,
            f"non_positive_{column}",
        )

    # 6. 检查成交量不能为负数
    reject(
        df["vol"] < 0,
        "negative_volume",
    )

    # 7. 检查最高价不能低于最低价
    reject(
        df["high"] < df["low"],
        "high_lower_than_low",
    )

    # 8. 检查开盘价、收盘价是否在最高价和最低价之间
    reject(
        (df["open"] > df["high"])
        | (df["open"] < df["low"]),
        "open_out_of_range",
    )

    reject(
        (df["close"] > df["high"])
        | (df["close"] < df["low"]),
        "close_out_of_range",
    )

    # 9. 检查重复交易日期
    # 同一只股票同一天只保留第一条记录
    valid_before_dedup = reject_reasons == ""

    duplicate_mask = (
        df.loc[valid_before_dedup]
        .duplicated(subset=["ts_code", "trade_date"], keep="first")
    )

    duplicate_indices = duplicate_mask[
        duplicate_mask
    ].index

    reject_reasons.loc[duplicate_indices] = "duplicate_trade_date"

    # 10. 分离合格数据和不合格数据
    rejected_mask = reject_reasons != ""

    clean_df = df.loc[~rejected_mask].copy()

    rejected_df = df.loc[rejected_mask].copy()
    rejected_df["reject_reason"] = reject_reasons.loc[
        rejected_mask
    ]

    # 11. 按交易日期升序排列
    clean_df = clean_df.sort_values(
        by="trade_date"
    ).reset_index(drop=True)

    # 12. 统计清洗结果
    reject_statistics = (
        rejected_df["reject_reason"]
        .value_counts()
        .to_dict()
    )

    quality_report = {
        "total_rows": original_count,
        "cleaned_rows": len(clean_df),
        "rejected_rows": len(rejected_df),
        "reject_rate": (
            len(rejected_df) / original_count
            if original_count > 0
            else 0
        ),
        "reject_reasons": reject_statistics,
    }

    return clean_df, rejected_df, quality_report


def clean_file(input_file: Path, output_dir: Path):
    """
    清洗单个 CSV 文件。
    """

    print(f"正在处理: {input_file}")

    df = pd.read_csv(input_file)

    clean_df, rejected_df, quality_report = validate_data(df)

    output_dir.mkdir(parents=True, exist_ok=True)

    # 输出清洗后的数据
    clean_file_path = output_dir / "cleaned.csv"
    clean_df.to_csv(
        clean_file_path,
        index=False,
        encoding="utf-8",
    )

    # 输出被剔除的数据
    rejected_file_path = output_dir / "rejected.csv"
    rejected_df.to_csv(
        rejected_file_path,
        index=False,
        encoding="utf-8",
    )

    # 获取实际数据范围
    if not clean_df.empty:
        start_date = clean_df["trade_date"].min()
        end_date = clean_df["trade_date"].max()
        stock_codes = clean_df["ts_code"].dropna().unique().tolist()
    else:
        start_date = None
        end_date = None
        stock_codes = []

    # 输出质量报告
    quality_report.update({
        "source_file": str(input_file.relative_to(PROJECT_ROOT)),
        "generated_at": datetime.now(timezone.utc).isoformat(),
    })

    with open(
        output_dir / "quality_report.json",
        "w",
        encoding="utf-8",
    ) as file:
        json.dump(
            quality_report,
            file,
            ensure_ascii=False,
            indent=4,
        )

    # 输出数据集元信息
    metadata = {
        "source": "Tushare",
        "source_file": input_file.name,
        "stock_codes": stock_codes,
        "start_date": start_date,
        "end_date": end_date,
        "columns": clean_df.columns.tolist(),
        "row_count": len(clean_df),
        "cleaning_script_version": SCRIPT_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
    }

    with open(
        output_dir / "metadata.json",
        "w",
        encoding="utf-8",
    ) as file:
        json.dump(
            metadata,
            file,
            ensure_ascii=False,
            indent=4,
        )

    print(f"清洗完成: {input_file.name}")
    print(f"原始行数: {quality_report['total_rows']}")
    print(f"合格行数: {quality_report['cleaned_rows']}")
    print(f"剔除行数: {quality_report['rejected_rows']}")
    print(f"输出目录: {output_dir}")


def main():
    parser = argparse.ArgumentParser(
        description="清洗原始行情 CSV 数据"
    )

    parser.add_argument(
        "--input",
        type=Path,
        default=None,
        help="指定一个原始 CSV 文件；不指定则扫描 raw 目录",
    )

    args = parser.parse_args()

    if args.input:
        input_file = args.input.resolve()

        if not input_file.is_file():
            raise FileNotFoundError(
                f"找不到输入文件: {input_file}"
            )

        if not input_file.is_relative_to(RAW_DIR.resolve()):
            raise ValueError(
                "输入文件必须位于 data/raw 目录内"
            )

        relative_path = input_file.relative_to(RAW_DIR.resolve())
        output_dir = CLEANED_DIR / relative_path.parent

        clean_file(input_file, output_dir)

    else:
        csv_files = list(RAW_DIR.rglob("*.csv"))

        if not csv_files:
            print(f"没有找到 CSV 文件: {RAW_DIR}")
            return

        for input_file in csv_files:
            relative_path = input_file.relative_to(RAW_DIR)
            output_dir = CLEANED_DIR / relative_path.parent

            try:
                clean_file(input_file, output_dir)
            except Exception as error:
                print(f"处理失败: {input_file}")
                print(f"原因: {error}")


if __name__ == "__main__":
    main()