use axum::{
    Router,
    routing::{delete, get, post},
};
pub use state::AppState;

mod decimal;
mod dto;
mod error;
mod handlers;
mod state;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route(
            "/markets/{base}/{quote}/book-ticker",
            get(handlers::book_ticker),
        )
        .route("/orders/limit", post(handlers::place_limit_order))
        .route("/orders/market", post(handlers::place_market_order))
        .route("/orders/stop-limit", post(handlers::place_stop_limit_order))
        .route("/balances/{asset}", get(handlers::get_balance))
        .route(
            "/markets/{base}/{quote}/orders/{order_id}",
            delete(handlers::cancel_limit_order),
        )
        .route("/admin/assets", post(handlers::create_asset))
        .route("/admin/markets", post(handlers::create_market))
        .route("/users", post(handlers::create_user))
        .route(
            "/admin/users/{user_id}/deposits",
            post(handlers::deposit_asset),
        )
        .with_state(state)
}
