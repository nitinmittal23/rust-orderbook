use sqlx::{FromRow, PgConnection, types::BigDecimal};

use crate::domain::primitives::{Price, Quantity};

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
