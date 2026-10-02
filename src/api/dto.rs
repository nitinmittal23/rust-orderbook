use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct BookTickerResponse {
    pub base: String,
    pub quote: String,
    pub best_bid: Option<String>,
    pub best_ask: Option<String>,
}

#[derive(Deserialize)]
pub struct PlaceLimitOrderRequest {
    pub base: String,
    pub quote: String,
    pub side: String,
    pub quantity: String,
    pub price: String,

    #[serde(default)]
    pub client_order_id: Option<String>,
}

#[derive(Serialize)]
pub struct OrderPlacementResponse {
    pub order_id: String,
    pub trades: Vec<TradeResponse>,
    pub unfilled_quantity: String,
    pub client_order_id: String,
}

#[derive(Serialize)]
pub struct TradeResponse {
    pub maker_order_id: String,
    pub taker_order_id: String,
    pub taker_side: String,
    pub price: String,
    pub quantity: String,
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub asset: String,
    pub available: String,
    pub locked: String,
}

#[derive(Serialize)]
pub struct CancelOrderResponse {
    pub order_id: String,
    pub remaining_quantity: String,
    pub original_quantity: String,
    pub side: String,
    pub price: String,
}

#[derive(Deserialize)]
#[serde(tag = "side", rename_all = "lowercase")]
pub enum PlaceMarketOrderRequest {
    Buy {
        base: String,
        quote: String,
        quantity: String,
        max_quote_amount: String,
        #[serde(default)]
        client_order_id: Option<String>,
    },
    Sell {
        base: String,
        quote: String,
        quantity: String,
        #[serde(default)]
        client_order_id: Option<String>,
    },
}

#[derive(Deserialize)]
pub struct PlaceStopLimitOrderRequest {
    pub base: String,
    pub quote: String,
    pub side: String,
    pub quantity: String,
    pub stop_price: String,
    pub limit_price: String,
    #[serde(default)]
    pub client_order_id: Option<String>,
}

#[derive(Serialize)]
pub struct PlaceStopLimitOrderResponse {
    pub order_id: String,
    pub client_order_id: String,
}

#[derive(Deserialize)]
pub struct CreateAssetRequest {
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
}

#[derive(Serialize)]
pub struct AssetResponse {
    pub id: i64,
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct CreateMarketRequest {
    pub base: String,
    pub quote: String,
    pub price_tick: String,
    pub quantity_step: String,
}

#[derive(Serialize)]
pub struct MarketResponse {
    pub id: i64,
    pub base: String,
    pub quote: String,
    pub price_tick: String,
    pub quantity_step: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub display_name: String,
    pub email: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: i64,
    pub display_name: String,
    pub email: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct DepositAssetRequest {
    pub asset: String,
    pub amount: String,
    pub reference_id: String,
}

#[derive(Serialize)]
pub struct DepositAssetResponse {
    pub id: String,
    pub reference_id: String,
    pub user_id: i64,
    pub asset: String,
    pub amount: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct MarketSummaryResponse {
    pub id: i64,
    pub base: String,
    pub quote: String,
    pub price_tick: String,
    pub quantity_step: String,
    pub last_trade_price: Option<String>,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct MarketsResponse {
    pub markets: Vec<MarketSummaryResponse>,
}

#[derive(Deserialize)]
pub struct MarketDepthQuery {
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct DepthLevelResponse {
    pub price: String,
    pub quantity: String,
}

#[derive(Serialize)]
pub struct MarketDepthResponse {
    pub base: String,
    pub quote: String,
    pub bids: Vec<DepthLevelResponse>,
    pub asks: Vec<DepthLevelResponse>,
}

#[derive(Deserialize)]
pub struct RecentTradesQuery {
    pub limit: Option<u32>,
}

#[derive(Serialize)]
pub struct RecentTradeResponse {
    pub trade_id: String,
    pub sequence: String,
    pub price: String,
    pub quantity: String,
    pub taker_side: String,
    pub executed_at: String,
}

#[derive(Serialize)]
pub struct RecentTradesResponse {
    pub base: String,
    pub quote: String,
    pub trades: Vec<RecentTradeResponse>,
}

#[derive(Deserialize)]
pub struct CandlesQuery {
    pub interval: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Serialize)]
pub struct CandleResponse {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

#[derive(Serialize)]
pub struct CandlesResponse {
    pub base: String,
    pub quote: String,
    pub interval: String,
    pub candles: Vec<CandleResponse>,
}
