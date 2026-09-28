CREATE TABLE trades (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    market_id BIGINT NOT NULL REFERENCES markets(id),
    trade_sequence BIGINT NOT NULL,

    maker_order_id BIGINT NOT NULL REFERENCES orders(id),
    taker_order_id BIGINT NOT NULL REFERENCES orders(id),

    maker_user_id BIGINT NOT NULL REFERENCES users(id),
    taker_user_id BIGINT NOT NULL REFERENCES users(id),

    taker_side TEXT NOT NULL,

    price_atomic NUMERIC(39, 0) NOT NULL,
    quantity_atomic NUMERIC(39, 0) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT trades_market_id_sequence_unique
        UNIQUE (market_id, trade_sequence),
    
    CONSTRAINT positive_trade_sequence
        CHECK (trade_sequence > 0),

    CONSTRAINT order_ids_differ
        CHECK (maker_order_id <> taker_order_id),

    CONSTRAINT taker_side_value
        CHECK (taker_side IN ('BUY', 'SELL')),

    CONSTRAINT positive_price_atomic
        CHECK (price_atomic > 0),

    CONSTRAINT positive_quantity_atomic
        CHECK (quantity_atomic > 0)
);