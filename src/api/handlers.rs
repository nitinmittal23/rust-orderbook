use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

use crate::{
    domain::{
        asset::AssetSymbol,
        order::Side,
        pair::TradingPair,
        primitives::{AssetAmount, OrderId, Price, Quantity, UserId},
    },
    exchange::MarketOrderRequest,
};

use super::{
    decimal::{format_decimal, parse_decimal},
    dto::{
        BalanceResponse, BookTickerResponse, CancelLimitOrderResponse, OrderPlacementResponse,
        PlaceLimitOrderRequest, PlaceMarketOrderRequest, TradeResponse,
    },
    error::ApiError,
    state::AppState,
};

pub async fn health() -> &'static str {
    "ok"
}

pub async fn book_ticker(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
) -> Result<Json<BookTickerResponse>, ApiError> {
    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let exchange = state.exchange.lock().map_err(|_| {
        ApiError::internal(
            "EXCHANGE_STATE_UNAVAILABLE",
            "exchange state is unavailable",
        )
    })?;

    let market = exchange
        .market(&pair)
        .ok_or_else(|| ApiError::not_found("MARKET_NOT_FOUND", "market not found"))?;

    let order_book = market.order_book();

    let quote_decimals = exchange
        .asset(pair.quote())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "quote asset configuration is unavailable",
            )
        })?
        .decimals();

    let best_bid = order_book
        .best_bid()
        .map(|price| format_response_decimal(price.value(), quote_decimals))
        .transpose()?;

    let best_ask = order_book
        .best_ask()
        .map(|price| format_response_decimal(price.value(), quote_decimals))
        .transpose()?;

    Ok(Json(BookTickerResponse {
        base: pair.base().as_str().to_string(),
        quote: pair.quote().as_str().to_string(),
        best_bid,
        best_ask,
    }))
}

fn user_id_from_header(headers: &HeaderMap) -> Result<UserId, ApiError> {
    let header = headers
        .get("x-user-id")
        .ok_or_else(|| ApiError::unauthorized("MISSING_USER_ID", "X-User-Id header is required"))?;

    let text = header
        .to_str()
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id header is invalid"))?;

    let value = text
        .parse::<u64>()
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id header is invalid"))?;

    Ok(UserId::new(value))
}

fn parse_side(value: &str) -> Result<Side, ApiError> {
    match value.to_ascii_lowercase().as_str() {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        _ => Err(ApiError::bad_request(
            "INVALID_SIDE",
            "side must be buy or sell",
        )),
    }
}

fn format_response_decimal(value: u128, decimals: u8) -> Result<String, ApiError> {
    format_decimal(value, decimals).map_err(|_| {
        ApiError::internal(
            "DECIMAL_FORMAT_ERROR",
            "response amount could not be formatted",
        )
    })
}

pub async fn place_limit_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlaceLimitOrderRequest>,
) -> Result<(StatusCode, Json<OrderPlacementResponse>), ApiError> {
    let user_id = user_id_from_header(&headers)?;

    let base_symbol = AssetSymbol::new(&request.base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&request.quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let side = parse_side(&request.side)?;

    let mut exchange = state.exchange.lock().map_err(|_| {
        ApiError::internal(
            "EXCHANGE_STATE_UNAVAILABLE",
            "exchange state is unavailable",
        )
    })?;

    if exchange.market(&pair).is_none() {
        return Err(ApiError::not_found("MARKET_NOT_FOUND", "market not found"));
    }

    let base_decimals = exchange
        .asset(pair.base())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "base asset configuration is unavailable",
            )
        })?
        .decimals();

    let quote_decimals = exchange
        .asset(pair.quote())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "quote asset configuration is unavailable",
            )
        })?
        .decimals();

    let quantity_atomic = parse_decimal(&request.quantity, base_decimals).map_err(|_| {
        ApiError::bad_request("INVALID_QUANTITY", "quantity has an invalid decimal")
    })?;

    let price_atomic = parse_decimal(&request.price, quote_decimals).map_err(|_| {
        ApiError::bad_request("INVALID_PRICE", "price has an invalid decimal format")
    })?;

    let quantity = Quantity::new(quantity_atomic);
    let price = Price::new(price_atomic)
        .map_err(|_| ApiError::bad_request("INVALID_PRICE", "price must be greater than zero"))?;

    let result = exchange.place_limit_order(user_id, &pair, side, price, quantity)?;

    drop(exchange);

    let trades = result
        .trades()
        .iter()
        .map(|trade| -> Result<TradeResponse, ApiError> {
            let taker_side = match trade.taker_side() {
                Side::Buy => "buy",
                Side::Sell => "sell",
            };

            Ok(TradeResponse {
                maker_order_id: trade.maker_order_id().value().to_string(),
                taker_order_id: trade.taker_order_id().value().to_string(),
                taker_side: taker_side.to_string(),
                price: format_response_decimal(trade.price().value(), quote_decimals)?,
                quantity: format_response_decimal(trade.quantity().value(), base_decimals)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    let response = OrderPlacementResponse {
        order_id: result.order_id().value().to_string(),
        trades,
        unfilled_quantity: format_response_decimal(
            result.unfilled_quantity().value(),
            base_decimals,
        )?,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn get_balance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<BalanceResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;

    let symbol = AssetSymbol::new(&asset)
        .map_err(|_| ApiError::bad_request("INVALID_ASSET_SYMBOL", "invalid asset symbol"))?;

    let exchange = state.exchange.lock().map_err(|_| {
        ApiError::internal(
            "EXCHANGE_STATE_UNAVAILABLE",
            "exchange state is unavailable",
        )
    })?;

    let decimals = exchange
        .asset(&symbol)
        .ok_or_else(|| ApiError::not_found("ASSET_NOT_FOUND", "asset not found"))?
        .decimals();

    let balance = exchange.ledger().balance(user_id, &symbol);

    drop(exchange);

    let response = BalanceResponse {
        asset: symbol.as_str().to_string(),
        available: format_response_decimal(balance.available().value(), decimals)?,
        locked: format_response_decimal(balance.locked().value(), decimals)?,
    };

    Ok(Json(response))
}

pub async fn cancel_limit_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((base, quote, order_id)): Path<(String, String, String)>,
) -> Result<Json<CancelLimitOrderResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;

    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let order_id_value = order_id
        .parse::<u64>()
        .map_err(|_| ApiError::bad_request("INVALID_ORDER_ID", "invalid order id"))?;
    let order_id = OrderId::new(order_id_value);

    let mut exchange = state.exchange.lock().map_err(|_| {
        ApiError::internal(
            "EXCHANGE_STATE_UNAVAILABLE",
            "exchange state is unavailable",
        )
    })?;

    if exchange.market(&pair).is_none() {
        return Err(ApiError::not_found("MARKET_NOT_FOUND", "market not found"));
    }

    let quote_decimals = exchange
        .asset(pair.quote())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "quote asset configuration is unavailable",
            )
        })?
        .decimals();

    let base_decimals = exchange
        .asset(pair.base())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "base asset configuration is unavailable",
            )
        })?
        .decimals();

    let result = exchange.cancel_order(user_id, &pair, order_id)?;

    let price = result.limit_price().ok_or_else(|| {
        ApiError::internal(
            "ORDER_CONFIGURATION_ERROR",
            "cancelled limit order has no price",
        )
    })?;
    drop(exchange);

    let response = CancelLimitOrderResponse {
        order_id: result.id().value().to_string(),
        price: format_response_decimal(price.value(), quote_decimals)?,
        original_quantity: format_response_decimal(
            result.original_quantity().value(),
            base_decimals,
        )?,
        remaining_quantity: format_response_decimal(
            result.remaining_quantity().value(),
            base_decimals,
        )?,
        side: match result.side() {
            Side::Buy => "buy".to_string(),
            Side::Sell => "sell".to_string(),
        },
    };

    Ok(Json(response))
}

pub async fn place_market_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlaceMarketOrderRequest>,
) -> Result<(StatusCode, Json<OrderPlacementResponse>), ApiError> {
    let user_id = user_id_from_header(&headers)?;

    let (base, quote) = match &request {
        PlaceMarketOrderRequest::Buy { base, quote, .. }
        | PlaceMarketOrderRequest::Sell { base, quote, .. } => (base, quote),
    };

    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let mut exchange = state.exchange.lock().map_err(|_| {
        ApiError::internal(
            "EXCHANGE_STATE_UNAVAILABLE",
            "exchange state is unavailable",
        )
    })?;

    if exchange.market(&pair).is_none() {
        return Err(ApiError::not_found("MARKET_NOT_FOUND", "market not found"));
    }

    let base_decimals = exchange
        .asset(pair.base())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "base asset configuration is unavailable",
            )
        })?
        .decimals();

    let quote_decimals = exchange
        .asset(pair.quote())
        .ok_or_else(|| {
            ApiError::internal(
                "ASSET_CONFIGURATION_ERROR",
                "quote asset configuration is unavailable",
            )
        })?
        .decimals();

    let market_request = match request {
        PlaceMarketOrderRequest::Buy {
            quantity,
            max_quote_amount,
            ..
        } => {
            let quantity = parse_decimal(&quantity, base_decimals).map_err(|_| {
                ApiError::bad_request("INVALID_QUANTITY", "quantity has an invalid decimal format")
            })?;
            let budget = parse_decimal(&max_quote_amount, quote_decimals).map_err(|_| {
                ApiError::bad_request(
                    "INVALID_MAX_QUOTE_AMOUNT",
                    "max quote amount has an invalid decimal format",
                )
            })?;
            MarketOrderRequest::Buy {
                quantity: Quantity::new(quantity),
                max_quote_amount: AssetAmount::new(budget),
            }
        }
        PlaceMarketOrderRequest::Sell { quantity, .. } => {
            let quantity = parse_decimal(&quantity, base_decimals).map_err(|_| {
                ApiError::bad_request("INVALID_QUANTITY", "quantity has an invalid decimal format")
            })?;
            MarketOrderRequest::Sell {
                quantity: Quantity::new(quantity),
            }
        }
    };

    let result = exchange.place_market_order(user_id, &pair, market_request)?;

    drop(exchange);

    let trades = result
        .trades()
        .iter()
        .map(|trade| -> Result<TradeResponse, ApiError> {
            let taker_side = match trade.taker_side() {
                Side::Buy => "buy",
                Side::Sell => "sell",
            };

            Ok(TradeResponse {
                maker_order_id: trade.maker_order_id().value().to_string(),
                taker_order_id: trade.taker_order_id().value().to_string(),
                taker_side: taker_side.to_string(),
                price: format_response_decimal(trade.price().value(), quote_decimals)?,
                quantity: format_response_decimal(trade.quantity().value(), base_decimals)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    let response = OrderPlacementResponse {
        order_id: result.order_id().value().to_string(),
        trades,
        unfilled_quantity: format_response_decimal(
            result.unfilled_quantity().value(),
            base_decimals,
        )?,
    };

    Ok((StatusCode::CREATED, Json(response)))
}
