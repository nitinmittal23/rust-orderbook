use sqlx::PgPool;

use crate::persistence::postgres::users::{self, UserRecord};

#[derive(Clone)]
pub struct UserService {
    db: PgPool,
}

#[derive(Debug)]
pub enum UserServiceError {
    InvalidDisplayName,
    InvalidEmail,
    EmailAlreadyExists,
    Database(sqlx::Error),
}

fn map_user_insert_error(error: sqlx::Error) -> UserServiceError {
    match &error {
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some("23505")
                && database_error.constraint() == Some("users_email_key") =>
        {
            UserServiceError::EmailAlreadyExists
        }

        _ => UserServiceError::Database(error),
    }
}

impl UserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn create_user(
        &self,
        display_name: &str,
        email: &str,
    ) -> Result<UserRecord, UserServiceError> {
        let display_name = display_name.trim();

        if display_name.is_empty() {
            return Err(UserServiceError::InvalidDisplayName);
        }

        let email = email.trim().to_ascii_lowercase();

        if email.is_empty() {
            return Err(UserServiceError::InvalidEmail);
        }

        let mut transaction = self.db.begin().await.map_err(UserServiceError::Database)?;

        let record = users::insert(transaction.as_mut(), display_name, &email)
            .await
            .map_err(map_user_insert_error)?;

        transaction
            .commit()
            .await
            .map_err(UserServiceError::Database)?;

        Ok(record)
    }
}
