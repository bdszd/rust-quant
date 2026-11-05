CREATE TABLE IF NOT EXISTS backtest_tasks (
    id TEXT PRIMARY KEY,
    dataset TEXT NOT NULL,
    strategy TEXT NOT NULL,
    short_window INTEGER,
    long_window INTEGER,
    job_json TEXT,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
    result_json TEXT,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    execution_duration_ms BIGINT
);

CREATE INDEX IF NOT EXISTS idx_backtest_tasks_status_created_at
    ON backtest_tasks (status, created_at);
