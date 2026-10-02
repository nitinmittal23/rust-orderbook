use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    application::{
        admin::AdminService, market_data::MarketDataService, trading::TradingService,
        user::UserService,
    },
    exchange::Exchange,
};

#[derive(Clone)]
pub struct AppState {
    pub(crate) exchange: Arc<Mutex<Exchange>>,
    pub(crate) db: PgPool,
    pub(crate) trading_service: TradingService,
    pub(crate) admin_service: AdminService,
    pub(crate) user_service: UserService,
    pub(crate) market_data_service: MarketDataService,
}

impl AppState {
    pub fn new(exchange: Exchange, db: PgPool) -> Self {
        let exchange = Arc::new(Mutex::new(exchange));
        let trading_service = TradingService::new(exchange.clone(), db.clone());
        let admin_service = AdminService::new(exchange.clone(), db.clone());
        let user_service = UserService::new(db.clone());
        let market_data_service = MarketDataService::new(db.clone());
        Self {
            exchange,
            db,
            trading_service,
            admin_service,
            user_service,
            market_data_service,
        }
    }
}
