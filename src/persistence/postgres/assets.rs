use sqlx::{FromRow, PgConnection};

use crate::domain::asset::{Asset, AssetSymbol};

#[derive(Debug, FromRow)]
pub struct AssetRecord {
    pub id: i64,
    pub symbol: String,
    pub name: String,
    pub decimals: i16,
    pub enabled: bool,
}

pub async fn find_by_symbol(
    connection: &mut PgConnection,
    symbol: &AssetSymbol,
) -> Result<Option<AssetRecord>, sqlx::Error> {
    sqlx::query_as::<_, AssetRecord>(
        r#"
        SELECT id, symbol, name, decimals, enabled
        FROM assets
        WHERE symbol = $1
        "#,
    )
    .bind(symbol.as_str())
    .fetch_optional(connection)
    .await
}

pub async fn insert(
    connection: &mut PgConnection,
    asset: &Asset,
    name: &str,
) -> Result<AssetRecord, sqlx::Error> {
    sqlx::query_as::<_, AssetRecord>(
        r#"
        INSERT into assets (symbol, name, decimals)
        VALUES ($1, $2, $3)
        RETURNING id, symbol, name, decimals, enabled
        "#,
    )
    .bind(asset.symbol().as_str())
    .bind(name)
    .bind(i16::from(asset.decimals()))
    .fetch_one(connection)
    .await
}

pub async fn list_all(connection: &mut PgConnection) -> Result<Vec<AssetRecord>, sqlx::Error> {
    sqlx::query_as::<_, AssetRecord>(
        r#"
        SELECT id, symbol, name, decimals, enabled
        FROM assets
        ORDER BY id ASC
        "#,
    )
    .fetch_all(connection)
    .await
}
