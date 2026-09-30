use sqlx::{PgPool, postgres::PgPoolOptions};

pub mod assets;
pub mod balance_movements;
pub mod balances;
pub mod markets;
pub mod orders;
pub mod trades;
pub mod users;

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}
