use std::sync::{Arc, Mutex};

use crate::exchange::Exchange;

#[derive(Clone)]
pub struct AppState {
    pub(crate) exchange: Arc<Mutex<Exchange>>,
}

impl AppState {
    pub fn new(exchange: Exchange) -> Self {
        Self {
            exchange: Arc::new(Mutex::new(exchange)),
        }
    }
}
