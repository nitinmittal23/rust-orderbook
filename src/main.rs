use orderbook::{
    api::{self, AppState},
    application::loader::load_exchange,
    persistence::postgres::create_pool,
};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let db = create_pool(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");
    let exchange = load_exchange(&db)
        .await
        .expect("failed to restore exchange");

    let state = AppState::new(exchange, db);
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind server address");

    axum::serve(listener, app).await.expect("server failed");
}
