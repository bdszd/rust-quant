# market-data

独立的 A 股市场数据模块。第一阶段支持 Tushare daily API 和 Tushare 原始 CSV。

## 模块边界

- Source：读取原始数据，不负责清洗、单位换算或写数据库。
- Normalize：统一日期和单位。
- Validation：检查 OHLC、成交量、成交额基本有效性。
- Storage：通过调用方注入的 PostgreSQL 连接池存储和查询行情。
- Service：提供数据集创建、行情导入和查询入口。
- 数据库迁移由 Backend 统一管理，SQL 文件位于 `apps/backend/migrations`；本 crate 不包含或运行独立迁移。

## 数据归属

- 每条行情必须属于一个 `dataset_id`。
- 数据集所有者分为平台（`platform`）和用户（`user`）。
- 平台数据集的 `owner_user_id` 为空；用户数据集必须关联用户 ID。
- 每个数据集独立保存行情。相同股票、相同交易日可以存在于不同数据集中，不会互相覆盖。
- 行情唯一键为 `(dataset_id, symbol, trade_date)`。
- 删除数据集时，其行情记录通过外键级联删除。
- 用户身份认证和数据集访问授权由 Backend 负责；本 crate 只执行数据集维度的读写，不替代 Backend 的所有权检查。

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
