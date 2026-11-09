CREATE TABLE IF NOT EXISTS market_symbols (
    symbol TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS market_daily_bars (
    symbol TEXT NOT NULL REFERENCES market_symbols(symbol) ON DELETE CASCADE,
    trade_date DATE NOT NULL,
    open NUMERIC(20, 6) NOT NULL,
    high NUMERIC(20, 6) NOT NULL,
    low NUMERIC(20, 6) NOT NULL,
    close NUMERIC(20, 6) NOT NULL,
    volume BIGINT NOT NULL CHECK (volume >= 0),
    amount NUMERIC(24, 4),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (symbol, trade_date)
);
CREATE INDEX IF NOT EXISTS idx_market_daily_bars_trade_date ON market_daily_bars(trade_date);
