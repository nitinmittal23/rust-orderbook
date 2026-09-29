use sqlx::{FromRow, PgConnection};

#[derive(Debug, FromRow)]
pub struct UserRecord {
    pub id: i64,
    pub display_name: String,
    pub email: String,
    pub enabled: bool,
}

pub async fn insert(
    connection: &mut PgConnection,
    display_name: &str,
    email: &str,
) -> Result<UserRecord, sqlx::Error> {
    sqlx::query_as::<_, UserRecord>(
        r#"
        INSERT INTO users (display_name, email)
        VALUES ($1, $2)
        RETURNING id, display_name, email, enabled
        "#,
    )
    .bind(display_name)
    .bind(email)
    .fetch_one(connection)
    .await
}

pub async fn find_by_id(
    connection: &mut PgConnection,
    user_id: i64,
) -> Result<Option<UserRecord>, sqlx::Error> {
    sqlx::query_as::<_, UserRecord>(
        r#"
        SELECT id, display_name, email, enabled
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(connection)
    .await
}
