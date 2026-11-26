-- 为现有用户增加平台角色。
-- 历史用户和未来新注册用户默认都是普通客户。
ALTER TABLE users
    ADD COLUMN role VARCHAR(20) NOT NULL DEFAULT 'customer';

ALTER TABLE users
    ADD CONSTRAINT chk_users_role
    CHECK (role IN ('customer', 'admin'));

CREATE INDEX idx_users_role
    ON users (role);
