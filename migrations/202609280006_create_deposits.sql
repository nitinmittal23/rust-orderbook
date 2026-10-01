CREATE TABLE deposits (
    id UUID PRIMARY KEY,
    reference_id TEXT NOT NULL UNIQUE,
    user_id BIGINT NOT NULL REFERENCES users(id),
    asset_id BIGINT NOT NULL REFERENCES assets(id),

    amount_atomic NUMERIC(39, 0) NOT NULL,
    status TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT deposits_reference_id_non_empty
        CHECK (LENGTH(BTRIM(reference_id)) > 0),

    CONSTRAINT deposits_amount_positive
        CHECK (amount_atomic > 0),

    CONSTRAINT deposits_status_value
        CHECK (status IN (
            'PENDING',
            'CREDITED',
            'REJECTED'
        ))
);
