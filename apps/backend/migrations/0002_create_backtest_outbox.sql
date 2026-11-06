CREATE TABLE IF NOT EXISTS backtest_outbox (
    task_id TEXT PRIMARY KEY REFERENCES backtest_tasks(id) ON DELETE CASCADE,
    payload TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_backtest_outbox_pending
    ON backtest_outbox (created_at)
    WHERE published_at IS NULL;
