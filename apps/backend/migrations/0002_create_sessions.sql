CREATE TABLE sessions (
    id UUID PRIMARY KEY,

    user_id UUID NOT NULL
        REFERENCES users(id)
        ON DELETE CASCADE,

    token_hash TEXT NOT NULL UNIQUE,

    expires_at TIMESTAMPTZ NOT NULL,

    revoked_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    last_used_at TIMESTAMPTZ
);

-- 查询用户的全部会话。
CREATE INDEX idx_sessions_user_id
    ON sessions(user_id);

-- 查询过期会话，方便后续清理。
CREATE INDEX idx_sessions_expires_at
    ON sessions(expires_at);

-- 查询用户尚未撤销的会话。
CREATE INDEX idx_sessions_active_user
    ON sessions(user_id)
    WHERE revoked_at IS NULL;