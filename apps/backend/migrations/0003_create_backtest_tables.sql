-- Preserve legacy tables from the pre-normalized backend.
-- The old schema used string task IDs and embedded job JSON. Keep those records
-- under explicit legacy names instead of colliding with the new UUID schema.
DO $
BEGIN
    IF to_regclass('public.backtest_tasks') IS NOT NULL
       AND (
           NOT EXISTS (
               SELECT 1 FROM information_schema.columns
               WHERE table_schema = 'public'
                 AND table_name = 'backtest_tasks'
                 AND column_name = 'user_id'
           )
           OR NOT EXISTS (
               SELECT 1 FROM information_schema.columns
               WHERE table_schema = 'public'
                 AND table_name = 'backtest_tasks'
                 AND column_name = 'id'
                 AND udt_name = 'uuid'
           )
       ) THEN
        IF to_regclass('public.backtest_tasks_legacy_v1') IS NOT NULL THEN
            RAISE EXCEPTION 'backtest_tasks_legacy_v1 already exists; inspect legacy data before migration';
        END IF;
        ALTER TABLE public.backtest_tasks RENAME TO backtest_tasks_legacy_v1;
    END IF;

    IF to_regclass('public.backtest_outbox') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1 FROM information_schema.columns
           WHERE table_schema = 'public'
             AND table_name = 'backtest_outbox'
             AND column_name = 'event_type'
       ) THEN
        IF to_regclass('public.backtest_outbox_legacy_v1') IS NOT NULL THEN
            RAISE EXCEPTION 'backtest_outbox_legacy_v1 already exists; inspect legacy data before migration';
        END IF;
        ALTER TABLE public.backtest_outbox RENAME TO backtest_outbox_legacy_v1;
    END IF;

    IF to_regclass('public.backtest_jobs') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1 FROM information_schema.columns
           WHERE table_schema = 'public'
             AND table_name = 'backtest_jobs'
             AND column_name = 'config'
       ) THEN
        IF to_regclass('public.backtest_jobs_legacy_v1') IS NOT NULL THEN
            RAISE EXCEPTION 'backtest_jobs_legacy_v1 already exists; inspect legacy data before migration';
        END IF;
        ALTER TABLE public.backtest_jobs RENAME TO backtest_jobs_legacy_v1;
    END IF;
END $;

-- Backtest persistence model.
--
-- Task is the user-visible request. Job is its immutable execution configuration.
-- Attempts record each execution; Result stores the final output; Outbox records
-- messages that must be delivered to RabbitMQ.

CREATE TABLE backtest_tasks (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'queued',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_backtest_tasks_status
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed'))
);

CREATE INDEX idx_backtest_tasks_created_at
    ON backtest_tasks (created_at DESC);

CREATE INDEX idx_backtest_tasks_user_created
    ON backtest_tasks (user_id, created_at DESC);

CREATE TABLE backtest_jobs (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL UNIQUE
        REFERENCES backtest_tasks(id) ON DELETE CASCADE,
    config JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE backtest_task_attempts (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL
        REFERENCES backtest_tasks(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    status VARCHAR(20) NOT NULL,
    worker_id VARCHAR(200),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_expires_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    duration_ms BIGINT CHECK (duration_ms IS NULL OR duration_ms >= 0),
    error TEXT,

    CONSTRAINT uq_backtest_attempt_number UNIQUE (task_id, attempt_number),
    CONSTRAINT chk_backtest_attempt_status
        CHECK (status IN ('running', 'succeeded', 'failed', 'interrupted'))
);

-- A task may have many historical attempts, but only one active attempt.
CREATE UNIQUE INDEX uq_backtest_one_running_attempt
    ON backtest_task_attempts (task_id)
    WHERE status = 'running';

CREATE INDEX idx_backtest_attempts_expired_lease
    ON backtest_task_attempts (lease_expires_at)
    WHERE status = 'running';

CREATE TABLE backtest_results (
    task_id UUID PRIMARY KEY
        REFERENCES backtest_tasks(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE backtest_outbox (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL
        REFERENCES backtest_tasks(id) ON DELETE CASCADE,
    event_type VARCHAR(100) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ,
    locked_by VARCHAR(200),
    locked_until TIMESTAMPTZ,
    publish_attempts INTEGER NOT NULL DEFAULT 0 CHECK (publish_attempts >= 0),
    last_error TEXT
);

CREATE INDEX idx_backtest_outbox_pending
    ON backtest_outbox (created_at)
    WHERE published_at IS NULL;
