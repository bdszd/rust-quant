-- ============================================================
-- 0001: Create Identity User Tables
--
-- 用户基本信息与认证凭据分离。
-- ============================================================

-- ------------------------------------------------------------
-- 1. users
-- ------------------------------------------------------------

CREATE TABLE users (
    id UUID PRIMARY KEY,

    username VARCHAR(50) NOT NULL UNIQUE,

    email VARCHAR(255) NOT NULL UNIQUE,

    status VARCHAR(20) NOT NULL DEFAULT 'pending',

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_users_status
        CHECK (status IN ('pending', 'active', 'disabled'))
);

-- 按用户状态查询。
CREATE INDEX idx_users_status
    ON users (status);

-- ------------------------------------------------------------
-- 2. user_credentials
-- ------------------------------------------------------------

CREATE TABLE user_credentials (
    user_id UUID PRIMARY KEY
        REFERENCES users(id)
        ON DELETE CASCADE,

    password_hash TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);