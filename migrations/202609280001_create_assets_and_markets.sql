CREATE TABLE assets (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    symbol TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    decimals SMALLINT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT assets_decimals_range
        CHECK (decimals BETWEEN 0 AND 18),
    
    CONSTRAINT assets_symbol_format
        CHECK (symbol ~ '^[A-Z0-9]+$')
);

CREATE TABLE markets (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    base_asset_id BIGINT NOT NULL 
        REFERENCES assets(id),
    quote_asset_id BIGINT NOT NULL 
        REFERENCES assets(id),
    price_tick_atomic NUMERIC(39, 0) NOT NULL,
    quantity_step_atomic NUMERIC(39, 0) NOT NULL,
    last_trade_price_atomic NUMERIC(39,0),

    next_order_sequence BIGINT NOT NULL DEFAULT 1,
    next_trade_sequence BIGINT NOT NULL DEFAULT 1,

    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT markets_assets_differ
        CHECK (base_asset_id <> quote_asset_id),
    
    CONSTRAINT market_pair_unique
        UNIQUE (base_asset_id, quote_asset_id),

    CONSTRAINT markets_price_tick_positive
        CHECK (price_tick_atomic > 0),

    CONSTRAINT markets_quantity_step_positive
        CHECK (quantity_step_atomic > 0),

    CONSTRAINT markets_last_trade_price_positive
        CHECK (
            last_trade_price_atomic IS NULL
            OR last_trade_price_atomic > 0
        ),

    CONSTRAINT markets_sequences_positive
        CHECK (
            next_order_sequence > 0
            AND next_trade_sequence > 0
        )
);

