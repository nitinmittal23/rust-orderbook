mod bootstrap;

use orderbook::api::{self, AppState};

#[tokio::main]
async fn main() {
    let exchange = bootstrap::create_development_exchange();
    let state = AppState::new(exchange);
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind server address");

    axum::serve(listener, app).await.expect("server failed");
}
