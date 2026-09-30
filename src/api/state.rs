use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{application::trading::TradingService, exchange::Exchange};

#[derive(Clone)]
pub struct AppState {
    pub(crate) exchange: Arc<Mutex<Exchange>>,
    pub(crate) trading_service: TradingService,
}

impl AppState {
    pub fn new(exchange: Exchange, db: PgPool) -> Self {
        let exchange = Arc::new(Mutex::new(exchange));
        let trading_service = TradingService::new(exchange.clone(), db.clone());
        Self {
            exchange,
            trading_service,
        }
    }
}
