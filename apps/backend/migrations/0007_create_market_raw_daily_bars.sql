-- ============================================================
-- 0007: Preserve source-level market data separately.
--
-- Raw rows are append-only snapshots. The normal market-data
-- queries continue to use market_daily_bars exclusively.
-- ============================================================

CREATE TABLE IF NOT EXISTS market_raw_daily_bars (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    dataset_id UUID NOT NULL
        REFERENCES market_datasets(id) ON DELETE CASCADE,
    source_type VARCHAR(40) NOT NULL,
    symbol TEXT NOT NULL,
    trade_date TEXT NOT NULL,
    raw_payload JSONB NOT NULL,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_market_raw_daily_bars_source
        CHECK (source_type IN ('tushare', 'csv', 'legacy'))
);

-- Supports occasional investigation by dataset, symbol and source date.
CREATE INDEX IF NOT EXISTS idx_market_raw_daily_bars_dataset_symbol_date
    ON market_raw_daily_bars (dataset_id, symbol, trade_date);

-- Supports tracing recent imports without burdening standard-bar queries.
CREATE INDEX IF NOT EXISTS idx_market_raw_daily_bars_ingested_at
    ON market_raw_daily_bars (ingested_at DESC);
