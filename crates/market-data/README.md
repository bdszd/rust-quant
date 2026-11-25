# market-data

独立的 A 股市场数据模块。第一阶段支持 Tushare daily API 和 Tushare 原始 CSV。

## 模块边界

- Source：读取原始数据，不负责清洗、单位换算或写数据库。
- Normalize：统一日期和单位。
- Validation：检查 OHLC、成交量、成交额基本有效性。
- Storage：PostgreSQL 幂等 upsert 与区间查询。
- Service：向调用方提供统一导入和查询入口。

本 crate 不依赖 backtest-core、trading-core 或 backend。

## 标准单位

- OHLC：元，使用 Decimal。
- volume：股，使用 i64。
- amount：元，使用 Decimal。
- Tushare vol 的单位为手，标准化时乘以 100。
- Tushare amount 的单位为千元，标准化时乘以 1000。

停牌日没有行情记录时不补造 Bar。CSV 第一版要求列：ts_code,trade_date,open,high,low,close,vol,amount。

## 测试

    cargo test -p market-data
    cargo check --workspace

调用方创建 TushareSource 或 CsvSource、PostgresRepository，再注入 MarketDataService。数据库连接池由调用方创建并注入，market-data 不自行管理独立连接池。
