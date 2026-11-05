import argparse
import os
import sys
from datetime import datetime
from pathlib import Path

import tushare as ts


# 项目根目录
PROJECT_ROOT = Path(__file__).resolve().parents[2]

# 原始数据目录
RAW_DATA_DIR = PROJECT_ROOT / "data" / "raw" / "tushare"

# 默认下载参数：不传命令行参数时，保持原来的下载行为
DEFAULT_STOCK_CODE = "000001.SZ"  # 平安银行
DEFAULT_START_DATE = "20240101"
DEFAULT_END_DATE = "20241231"

FIELDS = (
    "ts_code,"
    "trade_date,"
    "open,"
    "high,"
    "low,"
    "close,"
    "pre_close,"
    "change,"
    "pct_chg,"
    "vol,"
    "amount"
)


def parse_args():
    parser = argparse.ArgumentParser(
        description="从 Tushare 下载指定股票在指定日期范围内的原始日线行情数据。"
    )
    parser.add_argument(
        "--stock-code",
        default=DEFAULT_STOCK_CODE,
        help=f"股票代码，默认：{DEFAULT_STOCK_CODE}",
    )
    parser.add_argument(
        "--start-date",
        default=DEFAULT_START_DATE,
        help=f"开始日期，格式 YYYYMMDD，默认：{DEFAULT_START_DATE}",
    )
    parser.add_argument(
        "--end-date",
        default=DEFAULT_END_DATE,
        help=f"结束日期，格式 YYYYMMDD，默认：{DEFAULT_END_DATE}",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="目标 CSV 已存在时允许覆盖。",
    )

    args = parser.parse_args()

    # 检查日期格式
    try:
        start_date = datetime.strptime(args.start_date, "%Y%m%d")
        end_date = datetime.strptime(args.end_date, "%Y%m%d")
    except ValueError:
        parser.error("日期格式必须是 YYYYMMDD，例如 20240101。")

    if start_date > end_date:
        parser.error("--start-date 不能晚于 --end-date。")

    if not args.stock_code.strip():
        parser.error("--stock-code 不能为空。")

    return args


def main():
    args = parse_args()

    # 1. 读取 Token
    token = os.getenv("TUSHARE_TOKEN")
    if not token:
        print("错误：没有设置 TUSHARE_TOKEN 环境变量", file=sys.stderr)
        return 1

    # 2. 确定输出位置
    output_dir = RAW_DATA_DIR / args.stock_code
    output_file = output_dir / f"{args.start_date}_{args.end_date}.csv"

    # 默认不覆盖已有文件，避免重复执行时意外丢失原始数据
    if output_file.exists() and not args.overwrite:
        print(
            f"错误：目标文件已存在：{output_file}\n"
            "如确认需要重新下载并覆盖，请添加 --overwrite。",
            file=sys.stderr,
        )
        return 1

    print(f"开始下载 {args.stock_code}")
    print(f"时间范围：{args.start_date} - {args.end_date}")

    # 3. 初始化 Tushare 并请求历史日线行情
    try:
        ts.set_token(token)
        pro = ts.pro_api()

        df = pro.daily(
            ts_code=args.stock_code,
            start_date=args.start_date,
            end_date=args.end_date,
            fields=FIELDS,
        )
    except Exception as exc:
        print(
            f"错误：Tushare 请求失败（{type(exc).__name__}）：{exc}",
            file=sys.stderr,
        )
        return 1

    # 4. 检查结果
    if df is None or df.empty:
        print("错误：Tushare 没有返回行情数据。", file=sys.stderr)
        return 1

    # 5. 按交易日期升序排列
    # 这里只整理记录顺序，不做字段转换、单位换算或行情质量清洗
    df = df.sort_values("trade_date").reset_index(drop=True)

    # 6. 保存数据源返回的原始字段
    try:
        output_dir.mkdir(parents=True, exist_ok=True)
        df.to_csv(output_file, index=False)
    except OSError as exc:
        print(f"错误：保存 CSV 失败：{exc}", file=sys.stderr)
        return 1

    print("下载完成")
    print(f"数据条数：{len(df)}")
    print(f"保存位置：{output_file}")

    print("\n前五条数据：")
    print(df.head())

    print("\n最后五条数据：")
    print(df.tail())

    return 0


if __name__ == "__main__":
    sys.exit(main())
