use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::exchange::Exchange;

#[derive(Clone)]
pub struct AppState {
    pub(crate) exchange: Arc<Mutex<Exchange>>,
    pub(crate) db: PgPool,
}

impl AppState {
    pub fn new(exchange: Exchange, db: PgPool) -> Self {
        Self {
            exchange: Arc::new(Mutex::new(exchange)),
            db,
        }
    }
}
