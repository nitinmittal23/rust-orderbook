CREATE TABLE users (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    display_name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE balances (
    user_id BIGINT NOT NULL REFERENCES users(id),
    asset_id BIGINT NOT NULL REFERENCES assets(id),

    available_atomic NUMERIC(39, 0) NOT NULL DEFAULT 0,
    locked_atomic NUMERIC(39, 0) NOT NULL DEFAULT 0,

    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (user_id, asset_id),

    CONSTRAINT balances_available_nonnegative
        CHECK (available_atomic >= 0),
    CONSTRAINT balances_locked_nonnegative
        CHECK (locked_atomic >= 0)
);
