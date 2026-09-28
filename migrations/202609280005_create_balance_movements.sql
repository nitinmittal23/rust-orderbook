CREATE TABLE balance_movements (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    operation_id UUID NOT NULL,
    user_id BIGINT NOT NULL REFERENCES users(id),
    asset_id BIGINT NOT NULL REFERENCES assets(id),

    available_delta_atomic NUMERIC(39, 0) NOT NULL,
    locked_delta_atomic NUMERIC(39, 0) NOT NULL,

    movement_type TEXT NOT NULL,

    order_id BIGINT REFERENCES orders(id),
    trade_id BIGINT REFERENCES trades(id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT movement_type_value
        CHECK (movement_type IN (
            'DEPOSIT',
            'WITHDRAWAL',
            'ORDER_LOCK',
            'ORDER_UNLOCK',
            'TRADE_SETTLEMENT',
            'ADMIN_ADJUSTMENT'
        )),
    CONSTRAINT delta_non_zero
        CHECK(
            (available_delta_atomic <> 0) OR (locked_delta_atomic <> 0)
        )
);
