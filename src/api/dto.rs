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
}

#[derive(Serialize)]
pub struct OrderPlacementResponse {
    pub order_id: String,
    pub trades: Vec<TradeResponse>,
    pub unfilled_quantity: String,
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
pub struct CancelLimitOrderResponse {
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
    },
    Sell {
        base: String,
        quote: String,
        quantity: String,
    },
}
