-- ============================================================
-- 0006: Market data datasets and isolated daily bars
--
-- Backend is the single owner of database migrations.
-- Public and user-owned datasets store their bars independently.
-- ============================================================

CREATE TABLE IF NOT EXISTS market_symbols (
    symbol TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS market_datasets (
    id UUID PRIMARY KEY,
    name VARCHAR(120) NOT NULL,
    owner_type VARCHAR(20) NOT NULL,
    owner_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    source_type VARCHAR(40) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_market_datasets_owner_type
        CHECK (owner_type IN ('platform', 'user')),
    CONSTRAINT chk_market_datasets_owner
        CHECK (
            (owner_type = 'platform' AND owner_user_id IS NULL)
            OR
            (owner_type = 'user' AND owner_user_id IS NOT NULL)
        ),
    CONSTRAINT chk_market_datasets_status
        CHECK (status IN ('active', 'archived'))
);

CREATE INDEX IF NOT EXISTS idx_market_datasets_owner_created
    ON market_datasets (owner_type, owner_user_id, created_at DESC);

-- Upgrade a database on which the old market-data migration was applied:
-- preserve its existing bars by assigning them to one platform dataset.
DO $$
BEGIN
    IF to_regclass('public.market_daily_bars') IS NULL THEN
        CREATE TABLE market_daily_bars (
            dataset_id UUID NOT NULL
                REFERENCES market_datasets(id) ON DELETE CASCADE,
            symbol TEXT NOT NULL
                REFERENCES market_symbols(symbol) ON DELETE CASCADE,
            trade_date DATE NOT NULL,
            open NUMERIC(20, 6) NOT NULL,
            high NUMERIC(20, 6) NOT NULL,
            low NUMERIC(20, 6) NOT NULL,
            close NUMERIC(20, 6) NOT NULL,
            volume BIGINT NOT NULL CHECK (volume >= 0),
            amount NUMERIC(24, 4),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CONSTRAINT pk_market_daily_bars
                PRIMARY KEY (dataset_id, symbol, trade_date)
        );
    ELSE
        INSERT INTO market_datasets (
            id, name, owner_type, owner_user_id, source_type, status
        )
        VALUES (
            '00000000-0000-0000-0000-000000000001',
            '迁移前公共行情',
            'platform',
            NULL,
            'legacy',
            'active'
        )
        ON CONFLICT (id) DO NOTHING;

        ALTER TABLE market_daily_bars
            ADD COLUMN IF NOT EXISTS dataset_id UUID;

        UPDATE market_daily_bars
        SET dataset_id = '00000000-0000-0000-0000-000000000001'
        WHERE dataset_id IS NULL;

        ALTER TABLE market_daily_bars
            ALTER COLUMN dataset_id SET NOT NULL;

        ALTER TABLE market_daily_bars
            DROP CONSTRAINT IF EXISTS market_daily_bars_pkey;

        ALTER TABLE market_daily_bars
            ADD CONSTRAINT pk_market_daily_bars
            PRIMARY KEY (dataset_id, symbol, trade_date);

        ALTER TABLE market_daily_bars
            ADD CONSTRAINT fk_market_daily_bars_dataset
            FOREIGN KEY (dataset_id)
            REFERENCES market_datasets(id)
            ON DELETE CASCADE;
    END IF;
END;
$$;

CREATE INDEX IF NOT EXISTS idx_market_daily_bars_dataset_date
    ON market_daily_bars (dataset_id, trade_date);
