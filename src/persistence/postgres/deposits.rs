use sqlx::{FromRow, PgConnection, types::BigDecimal};
use uuid::Uuid;

#[derive(Debug)]
pub struct DepositInsert {
    pub id: Uuid,
    pub reference_id: String,
    pub user_id: i64,
    pub asset_id: i64,
    pub amount_atomic: BigDecimal,
    pub status: String,
}

#[derive(Debug, FromRow)]
pub struct DepositRecord {
    pub id: Uuid,
    pub reference_id: String,
    pub user_id: i64,
    pub asset_id: i64,
    pub amount_atomic: BigDecimal,
    pub status: String,
}

pub async fn insert_if_absent(
    connection: &mut PgConnection,
    deposit: &DepositInsert,
) -> Result<Option<DepositRecord>, sqlx::Error> {
    sqlx::query_as::<_, DepositRecord>(
        r#"
        INSERT INTO deposits (
            id,
            reference_id,
            user_id,
            asset_id,
            amount_atomic,
            status
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (reference_id) DO NOTHING
        RETURNING
            id,
            reference_id,
            user_id,
            asset_id,
            amount_atomic,
            status
        "#,
    )
    .bind(deposit.id)
    .bind(&deposit.reference_id)
    .bind(deposit.user_id)
    .bind(deposit.asset_id)
    .bind(&deposit.amount_atomic)
    .bind(&deposit.status)
    .fetch_optional(connection)
    .await
}

pub async fn find_by_reference(
    connection: &mut PgConnection,
    reference_id: &str,
) -> Result<Option<DepositRecord>, sqlx::Error> {
    sqlx::query_as::<_, DepositRecord>(
        r#"
        SELECT
            id,
            reference_id,
            user_id,
            asset_id,
            amount_atomic,
            status
        FROM deposits
        WHERE reference_id = $1
        "#,
    )
    .bind(reference_id)
    .fetch_optional(connection)
    .await
}
