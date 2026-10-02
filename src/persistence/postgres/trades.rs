use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, types::BigDecimal};

#[derive(Debug)]
pub struct TradeInsert {
    pub market_id: i64,
    pub trade_sequence: i64,
    pub maker_order_id: i64,
    pub taker_order_id: i64,
    pub maker_user_id: i64,
    pub taker_user_id: i64,
    pub taker_side: String,
    pub price_atomic: BigDecimal,
    pub quantity_atomic: BigDecimal,
}

#[derive(Debug, FromRow)]
pub struct TradeRecord {
    pub id: i64,
    pub market_id: i64,
    pub trade_sequence: i64,
    pub maker_order_id: i64,
    pub taker_order_id: i64,
    pub maker_user_id: i64,
    pub taker_user_id: i64,
    pub taker_side: String,
    pub price_atomic: BigDecimal,
    pub quantity_atomic: BigDecimal,
    pub created_at: DateTime<Utc>,
}

pub async fn insert(
    connection: &mut PgConnection,
    trade: &TradeInsert,
) -> Result<TradeRecord, sqlx::Error> {
    sqlx::query_as::<_, TradeRecord>(
        r#"
        INSERT INTO trades (
            market_id,
            trade_sequence,
            maker_order_id,
            taker_order_id,
            maker_user_id,
            taker_user_id,
            taker_side,
            price_atomic,
            quantity_atomic
        )
        VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8, $9
        )
        RETURNING
            id,
            market_id,
            trade_sequence,
            maker_order_id,
            taker_order_id,
            maker_user_id,
            taker_user_id,
            taker_side,
            price_atomic,
            quantity_atomic,
            created_at
        "#,
    )
    .bind(trade.market_id)
    .bind(trade.trade_sequence)
    .bind(trade.maker_order_id)
    .bind(trade.taker_order_id)
    .bind(trade.maker_user_id)
    .bind(trade.taker_user_id)
    .bind(&trade.taker_side)
    .bind(&trade.price_atomic)
    .bind(&trade.quantity_atomic)
    .fetch_one(connection)
    .await
}

pub async fn list_recent(
    connection: &mut PgConnection,
    market_id: i64,
    limit: i64,
) -> Result<Vec<TradeRecord>, sqlx::Error> {
    sqlx::query_as::<_, TradeRecord>(
        r#"
        SELECT
            id,
            market_id,
            trade_sequence,
            maker_order_id,
            taker_order_id,
            maker_user_id,
            taker_user_id,
            taker_side,
            price_atomic,
            quantity_atomic,
            created_at
        FROM trades
        WHERE market_id = $1
        ORDER BY trade_sequence DESC
        LIMIT $2
        "#,
    )
    .bind(market_id)
    .bind(limit)
    .fetch_all(connection)
    .await
}

pub async fn list_between(
    connection: &mut PgConnection,
    market_id: i64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<TradeRecord>, sqlx::Error> {
    sqlx::query_as::<_, TradeRecord>(
        r#"
        SELECT
            id,
            market_id,
            trade_sequence,
            maker_order_id,
            taker_order_id,
            maker_user_id,
            taker_user_id,
            taker_side,
            price_atomic,
            quantity_atomic,
            created_at
        FROM trades
        WHERE market_id = $1
            AND created_at >= $2
            AND created_at < $3
        ORDER BY trade_sequence ASC
        "#,
    )
    .bind(market_id)
    .bind(start)
    .bind(end)
    .fetch_all(connection)
    .await
}
