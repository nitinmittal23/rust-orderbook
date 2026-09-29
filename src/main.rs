mod bootstrap;

use orderbook::api::{self, AppState};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let exchange = bootstrap::create_development_exchange();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = orderbook::persistence::postgres::create_pool(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");

    let state = AppState::new(exchange, db);
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind server address");

    axum::serve(listener, app).await.expect("server failed");
}
