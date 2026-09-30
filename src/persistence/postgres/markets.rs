use sqlx::{FromRow, PgConnection, types::BigDecimal};

use crate::domain::primitives::{Price, Quantity};

#[derive(Debug)]
pub struct MarketStateUpdate {
    pub id: i64,
    pub last_trade_price_atomic: Option<BigDecimal>,
    pub next_order_sequence: i64,
    pub next_trade_sequence: i64,
}

#[derive(Debug, FromRow)]
pub struct MarketRecord {
    pub id: i64,
    pub base_asset_id: i64,
    pub quote_asset_id: i64,
    pub price_tick_atomic: BigDecimal,
    pub quantity_step_atomic: BigDecimal,
    pub last_trade_price_atomic: Option<BigDecimal>,
    pub next_order_sequence: i64,
    pub next_trade_sequence: i64,
    pub enabled: bool,
}

pub async fn insert(
    connection: &mut PgConnection,
    base_asset_id: i64,
    quote_asset_id: i64,
    price_tick: Price,
    quantity_step: Quantity,
) -> Result<MarketRecord, sqlx::Error> {
    sqlx::query_as::<_, MarketRecord>(
        r#"
        INSERT into markets (base_asset_id, quote_asset_id, price_tick_atomic, quantity_step_atomic)
        VALUES ($1, $2, $3, $4)
        RETURNING id,
            base_asset_id,
            quote_asset_id,
            price_tick_atomic,
            quantity_step_atomic,
            last_trade_price_atomic,
            next_order_sequence,
            next_trade_sequence,
            enabled
        "#,
    )
    .bind(base_asset_id)
    .bind(quote_asset_id)
    .bind(BigDecimal::from(price_tick.value()))
    .bind(BigDecimal::from(quantity_step.value()))
    .fetch_one(connection)
    .await
}

pub async fn update(
    connection: &mut PgConnection,
    market: &MarketStateUpdate,
) -> Result<MarketRecord, sqlx::Error> {
    sqlx::query_as::<_, MarketRecord>(
        r#"
        UPDATE markets 
        SET
            last_trade_price_atomic = $2,
            next_order_sequence = $3,
            next_trade_sequence = $4
        WHERE id = $1
        RETURNING
            id,
            base_asset_id,
            quote_asset_id,
            price_tick_atomic,
            quantity_step_atomic,
            last_trade_price_atomic,
            next_order_sequence,
            next_trade_sequence,
            enabled
        "#,
    )
    .bind(market.id)
    .bind(&market.last_trade_price_atomic)
    .bind(market.next_order_sequence)
    .bind(market.next_trade_sequence)
    .fetch_one(connection)
    .await
}

pub async fn find_by_assets(
    connection: &mut PgConnection,
    base_asset_id: i64,
    quote_asset_id: i64,
) -> Result<Option<MarketRecord>, sqlx::Error> {
    sqlx::query_as::<_, MarketRecord>(
        r#"
        SELECT
            id,
            base_asset_id,
            quote_asset_id,
            price_tick_atomic,
            quantity_step_atomic,
            last_trade_price_atomic,
            next_order_sequence,
            next_trade_sequence,
            enabled
        FROM markets
        WHERE base_asset_id = $1
          AND quote_asset_id = $2
        "#,
    )
    .bind(base_asset_id)
    .bind(quote_asset_id)
    .fetch_optional(connection)
    .await
}

pub async fn list_all(connection: &mut PgConnection) -> Result<Vec<MarketRecord>, sqlx::Error> {
    sqlx::query_as::<_, MarketRecord>(
        r#"
        SELECT
            id,
            base_asset_id,
            quote_asset_id,
            price_tick_atomic,
            quantity_step_atomic,
            last_trade_price_atomic,
            next_order_sequence,
            next_trade_sequence,
            enabled
        FROM markets
        ORDER BY id ASC
        "#,
    )
    .fetch_all(connection)
    .await
}
