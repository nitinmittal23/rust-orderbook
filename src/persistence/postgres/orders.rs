use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, types::BigDecimal};

#[derive(Debug)]
pub struct OrderInsert {
    pub id: i64,
    pub client_order_id: String,
    pub market_id: i64,
    pub user_id: i64,
    pub side: String,
    pub status: String,
    pub kind: String,
    pub original_quantity_atomic: BigDecimal,
    pub remaining_quantity_atomic: BigDecimal,
    pub limit_price_atomic: Option<BigDecimal>,
    pub stop_price_atomic: Option<BigDecimal>,
    pub max_quote_amount_atomic: Option<BigDecimal>,
    pub sequence_number: Option<i64>,
    pub triggered_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct OrderStateUpdate {
    pub id: i64,
    pub status: String,
    pub remaining_quantity_atomic: BigDecimal,
    pub sequence_number: Option<i64>,
}

#[derive(Debug, FromRow)]
pub struct OrderRecord {
    pub id: i64,
    pub client_order_id: String,
    pub market_id: i64,
    pub user_id: i64,
    pub side: String,
    pub status: String,
    pub kind: String,
    pub original_quantity_atomic: BigDecimal,
    pub remaining_quantity_atomic: BigDecimal,
    pub limit_price_atomic: Option<BigDecimal>,
    pub stop_price_atomic: Option<BigDecimal>,
    pub max_quote_amount_atomic: Option<BigDecimal>,
    pub sequence_number: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub triggered_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
}

pub async fn insert(
    connection: &mut PgConnection,
    order: &OrderInsert,
) -> Result<OrderRecord, sqlx::Error> {
    sqlx::query_as::<_, OrderRecord>(
        r#"
        INSERT INTO orders (
            id,
            client_order_id,
            market_id,
            user_id,
            side,
            kind,
            status,
            original_quantity_atomic,
            remaining_quantity_atomic,
            limit_price_atomic,
            stop_price_atomic,
            max_quote_amount_atomic,
            sequence_number,
            triggered_at,
            closed_at
        )
        VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8, $9, $10,
            $11, $12, $13, $14, $15
        )
        RETURNING
            id,
            client_order_id,
            market_id,
            user_id,
            side,
            kind,
            status,
            original_quantity_atomic,
            remaining_quantity_atomic,
            limit_price_atomic,
            stop_price_atomic,
            max_quote_amount_atomic,
            sequence_number,
            created_at,
            updated_at,
            triggered_at,
            closed_at
        "#,
    )
    .bind(order.id)
    .bind(&order.client_order_id)
    .bind(order.market_id)
    .bind(order.user_id)
    .bind(&order.side)
    .bind(&order.kind)
    .bind(&order.status)
    .bind(&order.original_quantity_atomic)
    .bind(&order.remaining_quantity_atomic)
    .bind(&order.limit_price_atomic)
    .bind(&order.stop_price_atomic)
    .bind(&order.max_quote_amount_atomic)
    .bind(order.sequence_number)
    .bind(order.triggered_at)
    .bind(order.closed_at)
    .fetch_one(connection)
    .await
}

pub async fn update(
    connection: &mut PgConnection,
    order: &OrderStateUpdate,
) -> Result<OrderRecord, sqlx::Error> {
    sqlx::query_as::<_, OrderRecord>(
        r#"
        UPDATE orders 
        SET
            triggered_at = CASE
                WHEN kind = 'STOP_LIMIT'
                AND status = 'PENDING_TRIGGER'
                AND $2 <> 'PENDING_TRIGGER'
                THEN COALESCE(triggered_at, NOW())
                ELSE triggered_at
            END,
            closed_at = CASE
                WHEN $2 IN ('FILLED', 'CANCELLED', 'EXPIRED')
                THEN COALESCE(closed_at, NOW())
                ELSE closed_at
            END,
            status = $2,
            remaining_quantity_atomic = $3,
            sequence_number = $4,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            client_order_id,
            market_id,
            user_id,
            side,
            kind,
            status,
            original_quantity_atomic,
            remaining_quantity_atomic,
            limit_price_atomic,
            stop_price_atomic,
            max_quote_amount_atomic,
            sequence_number,
            created_at,
            updated_at,
            triggered_at,
            closed_at
        "#,
    )
    .bind(order.id)
    .bind(&order.status)
    .bind(&order.remaining_quantity_atomic)
    .bind(order.sequence_number)
    .fetch_one(connection)
    .await
}

pub async fn list_active(connection: &mut PgConnection) -> Result<Vec<OrderRecord>, sqlx::Error> {
    sqlx::query_as::<_, OrderRecord>(
        r#"
        SELECT
            id,
            client_order_id,
            market_id,
            user_id,
            side,
            kind,
            status,
            original_quantity_atomic,
            remaining_quantity_atomic,
            limit_price_atomic,
            stop_price_atomic,
            max_quote_amount_atomic,
            sequence_number,
            created_at,
            updated_at,
            triggered_at,
            closed_at
        FROM orders
        WHERE status IN ('OPEN', 'PARTIALLY_FILLED')
        ORDER BY market_id ASC, sequence_number ASC
        "#,
    )
    .fetch_all(connection)
    .await
}

pub async fn list_pending_stops(
    connection: &mut PgConnection,
) -> Result<Vec<OrderRecord>, sqlx::Error> {
    sqlx::query_as::<_, OrderRecord>(
        r#"
        SELECT
            id,
            client_order_id,
            market_id,
            user_id,
            side,
            kind,
            status,
            original_quantity_atomic,
            remaining_quantity_atomic,
            limit_price_atomic,
            stop_price_atomic,
            max_quote_amount_atomic,
            sequence_number,
            created_at,
            updated_at,
            triggered_at,
            closed_at
        FROM orders
        WHERE status = 'PENDING_TRIGGER'
        ORDER BY market_id ASC, id ASC
        "#,
    )
    .fetch_all(connection)
    .await
}

pub async fn max_id(connection: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(MAX(id), 0)
        FROM orders
        "#,
    )
    .fetch_one(connection)
    .await
}
