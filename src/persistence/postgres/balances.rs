use sqlx::{FromRow, PgConnection, types::BigDecimal};

use crate::domain::primitives::AssetAmount;

#[derive(Debug, FromRow)]
pub struct BalanceRecord {
    pub user_id: i64,
    pub asset_id: i64,
    pub available_atomic: BigDecimal,
    pub locked_atomic: BigDecimal,
}

pub async fn upsert(
    connection: &mut PgConnection,
    user_id: i64,
    asset_id: i64,
    available: AssetAmount,
    locked: AssetAmount,
) -> Result<BalanceRecord, sqlx::Error> {
    sqlx::query_as::<_, BalanceRecord>(
        r#"
        INSERT into balances (user_id, asset_id, available_atomic, locked_atomic)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (user_id, asset_id)
        DO UPDATE SET
            available_atomic = EXCLUDED.available_atomic,
            locked_atomic = EXCLUDED.locked_atomic,
            updated_at = NOW()
        RETURNING user_id, asset_id, available_atomic, locked_atomic
        "#,
    )
    .bind(user_id)
    .bind(asset_id)
    .bind(BigDecimal::from(available.value()))
    .bind(BigDecimal::from(locked.value()))
    .fetch_one(connection)
    .await
}
