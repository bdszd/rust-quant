-- Prevent the same source snapshot from being appended twice.
CREATE TABLE IF NOT EXISTS market_data_import_batches (
    batch_key TEXT PRIMARY KEY,
    dataset_id UUID NOT NULL
        REFERENCES market_datasets(id) ON DELETE CASCADE,
    raw_sha256 CHAR(64) NOT NULL,
    bars_sha256 CHAR(64) NOT NULL,
    raw_rows BIGINT NOT NULL CHECK (raw_rows >= 0),
    standard_rows BIGINT NOT NULL CHECK (standard_rows >= 0),
    first_trade_date DATE NOT NULL,
    last_trade_date DATE NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_market_data_import_batches_dataset
    ON market_data_import_batches (dataset_id, imported_at DESC);
