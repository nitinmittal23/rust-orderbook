use sqlx::{FromRow, PgConnection, types::BigDecimal};
use uuid::Uuid;

#[derive(Debug)]
pub struct BalanceMovementInsert {
    pub operation_id: Uuid,
    pub user_id: i64,
    pub asset_id: i64,
    pub available_delta_atomic: BigDecimal,
    pub locked_delta_atomic: BigDecimal,
    pub movement_type: String,
    pub order_id: Option<i64>,
    pub trade_id: Option<i64>,
}

#[derive(Debug, FromRow)]
pub struct BalanceMovementRecord {
    pub id: i64,
    pub operation_id: Uuid,
    pub user_id: i64,
    pub asset_id: i64,
    pub available_delta_atomic: BigDecimal,
    pub locked_delta_atomic: BigDecimal,
    pub movement_type: String,
    pub order_id: Option<i64>,
    pub trade_id: Option<i64>,
}

pub async fn insert(
    connection: &mut PgConnection,
    movement: &BalanceMovementInsert,
) -> Result<BalanceMovementRecord, sqlx::Error> {
    sqlx::query_as::<_, BalanceMovementRecord>(
        r#"
        INSERT INTO balance_movements (
            operation_id,
            user_id,
            asset_id,
            available_delta_atomic,
            locked_delta_atomic,
            movement_type,
            order_id,
            trade_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING
            id,
            operation_id,
            user_id,
            asset_id,
            available_delta_atomic,
            locked_delta_atomic,
            movement_type,
            order_id,
            trade_id
        "#,
    )
    .bind(movement.operation_id)
    .bind(movement.user_id)
    .bind(movement.asset_id)
    .bind(&movement.available_delta_atomic)
    .bind(&movement.locked_delta_atomic)
    .bind(&movement.movement_type)
    .bind(movement.order_id)
    .bind(movement.trade_id)
    .fetch_one(connection)
    .await
}
