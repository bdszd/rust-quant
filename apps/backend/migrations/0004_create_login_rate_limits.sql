-- Shared login attempt counters. Keys are SHA-256 digests; raw identifiers and IPs are never stored.
CREATE TABLE login_rate_limits (
    bucket_key CHAR(64) PRIMARY KEY,
    window_started_at TIMESTAMPTZ NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts > 0)
);

CREATE INDEX idx_login_rate_limits_window_started_at
    ON login_rate_limits(window_started_at);
