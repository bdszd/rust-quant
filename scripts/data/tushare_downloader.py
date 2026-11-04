
import os
import sys
from pathlib import Path

import tushare as ts


# 项目根目录
PROJECT_ROOT = Path(__file__).resolve().parents[2]

# 原始数据目录
RAW_DATA_DIR = PROJECT_ROOT / "data" / "raw" / "tushare"

# 下载参数
STOCK_CODE = "000001.SZ"  # 平安银行
START_DATE = "20240101"
END_DATE = "20241231"

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


def main():
    # 1. 读取 Token
    token = os.getenv("TUSHARE_TOKEN")

    if not token:
        print("错误：没有设置 TUSHARE_TOKEN 环境变量")
        sys.exit(1)

    # 2. 初始化 Tushare
    ts.set_token(token)
    pro = ts.pro_api()

    # 3. 创建存储目录
    output_dir = RAW_DATA_DIR / STOCK_CODE
    output_dir.mkdir(parents=True, exist_ok=True)

    output_file = output_dir / f"{START_DATE}_{END_DATE}.csv"

    print(f"开始下载 {STOCK_CODE}")
    print(f"时间范围：{START_DATE} - {END_DATE}")

    # 4. 请求历史日线行情
    df = pro.daily(
        ts_code=STOCK_CODE,
        start_date=START_DATE,
        end_date=END_DATE,
        fields=FIELDS
    )

    # 5. 检查结果
    if df is None or df.empty:
        print("没有获取到数据")
        sys.exit(1)

    # 6. 按交易日期升序排列
    df = df.sort_values("trade_date").reset_index(drop=True)

    # 7. 保存原始数据
    df.to_csv(output_file, index=False)

    print("下载完成")
    print(f"数据条数：{len(df)}")
    print(f"保存位置：{output_file}")

    print("\n前五条数据：")
    print(df.head())

    print("\n最后五条数据：")
    print(df.tail())


if __name__ == "__main__":
    main()