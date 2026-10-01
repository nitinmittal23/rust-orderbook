use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    domain::{
        asset::{Asset, AssetSymbol},
        order::Side,
        pair::TradingPair,
        primitives::{AssetAmount, OrderId, Price, Quantity, UserId},
    },
    exchange::MarketOrderRequest,
};

use super::{
    decimal::{format_decimal, parse_decimal},
    dto::{
        AssetResponse, BalanceResponse, BookTickerResponse, CancelOrderResponse,
        CreateAssetRequest, CreateMarketRequest, CreateUserRequest, DepositAssetRequest,
        DepositAssetResponse, MarketResponse, OrderPlacementResponse, PlaceLimitOrderRequest,
        PlaceMarketOrderRequest, PlaceStopLimitOrderRequest, PlaceStopLimitOrderResponse,
        TradeResponse, UserResponse,
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

    let exchange = state.exchange.lock().await;

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

fn resolve_client_order_id(value: Option<&str>) -> Result<String, ApiError> {
    match value {
        Some(value) => {
            let value = value.trim();

            if value.is_empty() {
                return Err(ApiError::bad_request(
                    "INVALID_CLIENT_ORDER_ID",
                    "client order ID cannot be empty",
                ));
            }

            if value.len() > 64 {
                return Err(ApiError::bad_request(
                    "INVALID_CLIENT_ORDER_ID",
                    "client order ID cannot exceed 64 characters",
                ));
            }

            Ok(value.to_string())
        }
        None => Ok(Uuid::new_v4().to_string()),
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
    let client_order_id = resolve_client_order_id(request.client_order_id.as_deref())?;

    let user_id = user_id_from_header(&headers)?;

    let base_symbol = AssetSymbol::new(&request.base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&request.quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let side = parse_side(&request.side)?;

    let (base_decimals, quote_decimals) = {
        let exchange = state.exchange.lock().await;

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

        (base_decimals, quote_decimals)
    };

    let quantity_atomic = parse_decimal(&request.quantity, base_decimals).map_err(|_| {
        ApiError::bad_request("INVALID_QUANTITY", "quantity has an invalid decimal")
    })?;

    let price_atomic = parse_decimal(&request.price, quote_decimals).map_err(|_| {
        ApiError::bad_request("INVALID_PRICE", "price has an invalid decimal format")
    })?;

    let quantity = Quantity::new(quantity_atomic);
    let price = Price::new(price_atomic)
        .map_err(|_| ApiError::bad_request("INVALID_PRICE", "price must be greater than zero"))?;

    let result = state
        .trading_service
        .place_limit_order(
            client_order_id.clone(),
            user_id,
            pair,
            side,
            quantity,
            price,
        )
        .await
        .map_err(ApiError::from)?;

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
        client_order_id,
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

    let exchange = state.exchange.lock().await;

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
) -> Result<Json<CancelOrderResponse>, ApiError> {
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

    let mut exchange = state.exchange.lock().await;

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
    let cancelled_order = result.cancelled_order();

    let price = cancelled_order.limit_price().ok_or_else(|| {
        ApiError::internal(
            "ORDER_CONFIGURATION_ERROR",
            "cancelled limit order has no price",
        )
    })?;
    drop(exchange);

    let response = CancelOrderResponse {
        order_id: cancelled_order.id().value().to_string(),
        price: format_response_decimal(price.value(), quote_decimals)?,
        original_quantity: format_response_decimal(
            cancelled_order.original_quantity().value(),
            base_decimals,
        )?,
        remaining_quantity: format_response_decimal(
            cancelled_order.remaining_quantity().value(),
            base_decimals,
        )?,
        side: match cancelled_order.side() {
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
    let client_order_id = match &request {
        PlaceMarketOrderRequest::Buy {
            client_order_id, ..
        }
        | PlaceMarketOrderRequest::Sell {
            client_order_id, ..
        } => resolve_client_order_id(client_order_id.as_deref())?,
    };

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

    let mut exchange = state.exchange.lock().await;

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
        client_order_id,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn place_stop_limit_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PlaceStopLimitOrderRequest>,
) -> Result<(StatusCode, Json<PlaceStopLimitOrderResponse>), ApiError> {
    let user_id = user_id_from_header(&headers)?;
    let client_order_id = resolve_client_order_id(request.client_order_id.as_deref())?;

    let base_symbol = AssetSymbol::new(&request.base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&request.quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let side = parse_side(&request.side)?;

    let mut exchange = state.exchange.lock().await;

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

    let stop_price_atomic = parse_decimal(&request.stop_price, quote_decimals).map_err(|_| {
        ApiError::bad_request(
            "INVALID_STOP_PRICE",
            "stop price has an invalid decimal format",
        )
    })?;

    let limit_price_atomic = parse_decimal(&request.limit_price, quote_decimals).map_err(|_| {
        ApiError::bad_request(
            "INVALID_LIMIT_PRICE",
            "limit price has an invalid decimal format",
        )
    })?;

    let quantity = Quantity::new(quantity_atomic);
    let stop_price = Price::new(stop_price_atomic).map_err(|_| {
        ApiError::bad_request("INVALID_STOP_PRICE", "stop price must be greater than zero")
    })?;
    let limit_price = Price::new(limit_price_atomic).map_err(|_| {
        ApiError::bad_request(
            "INVALID_LIMIT_PRICE",
            "limit price must be greater than zero",
        )
    })?;

    let result =
        exchange.place_stop_limit_order(user_id, &pair, side, stop_price, limit_price, quantity)?;

    drop(exchange);

    let response = PlaceStopLimitOrderResponse {
        order_id: result.order_id().value().to_string(),
        client_order_id,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn create_asset(
    State(state): State<AppState>,
    Json(request): Json<CreateAssetRequest>,
) -> Result<(StatusCode, Json<AssetResponse>), ApiError> {
    let symbol = AssetSymbol::new(&request.symbol)
        .map_err(|_| ApiError::bad_request("INVALID_ASSET_SYMBOL", "invalid asset symbol"))?;
    let asset = Asset::new(symbol, request.decimals).map_err(|_| {
        ApiError::bad_request(
            "INVALID_ASSET_DECIMALS",
            "asset decimal must be between 0 and 18",
        )
    })?;

    let record = state
        .admin_service
        .create_asset(asset, &request.name)
        .await
        .map_err(ApiError::from)?;

    let decimals = u8::try_from(record.decimals).map_err(|_| {
        ApiError::internal("INVALID_STORED_ASSET", "stored asset decimals are invalid")
    })?;

    let response = AssetResponse {
        id: record.id,
        symbol: record.symbol,
        name: record.name,
        decimals,
        enabled: record.enabled,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn create_market(
    State(state): State<AppState>,
    Json(request): Json<CreateMarketRequest>,
) -> Result<(StatusCode, Json<MarketResponse>), ApiError> {
    let base_symbol = AssetSymbol::new(&request.base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&request.quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let (base_decimals, quote_decimals) = {
        let exchange = state.exchange.lock().await;

        let base_decimals = exchange
            .asset(pair.base())
            .ok_or_else(|| ApiError::not_found("BASE_ASSET_NOT_FOUND", "base asset not found"))?
            .decimals();

        let quote_decimals = exchange
            .asset(pair.quote())
            .ok_or_else(|| ApiError::not_found("QUOTE_ASSET_NOT_FOUND", "quote asset not found"))?
            .decimals();

        (base_decimals, quote_decimals)
    };

    let price_tick_atomic = parse_decimal(&request.price_tick, quote_decimals)
        .map_err(|_| ApiError::bad_request("INVALID_PRICE_TICK", "invalid price tick"))?;

    let quantity_step_atomic = parse_decimal(&request.quantity_step, base_decimals)
        .map_err(|_| ApiError::bad_request("INVALID_QUANTITY_STEP", "invalid quantity step"))?;

    let price_tick = Price::new(price_tick_atomic).map_err(|_| {
        ApiError::bad_request("INVALID_PRICE_TICK", "price tick must be greater than zero")
    })?;

    let quantity_step = Quantity::new(quantity_step_atomic);

    if quantity_step.is_zero() {
        return Err(ApiError::bad_request(
            "INVALID_QUANTITY_STEP",
            "quantity step must be greater than zero",
        ));
    }

    let record = state
        .admin_service
        .create_market(pair.clone(), price_tick, quantity_step)
        .await
        .map_err(ApiError::from)?;

    let response = MarketResponse {
        id: record.id,
        base: pair.base().as_str().to_string(),
        quote: pair.quote().as_str().to_string(),
        price_tick: format_response_decimal(price_tick.value(), quote_decimals)?,
        quantity_step: format_response_decimal(quantity_step.value(), base_decimals)?,
        enabled: record.enabled,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    let record = state
        .user_service
        .create_user(&request.display_name, &request.email)
        .await
        .map_err(ApiError::from)?;

    let response = UserResponse {
        id: record.id,
        display_name: record.display_name,
        email: record.email,
        enabled: record.enabled,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn deposit_asset(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(request): Json<DepositAssetRequest>,
) -> Result<(StatusCode, Json<DepositAssetResponse>), ApiError> {
    let user_id_value = user_id
        .parse::<u64>()
        .map_err(|_| ApiError::bad_request("INVALID_USER_ID", "invalid user ID"))?;

    let user_id = UserId::new(user_id_value);

    let asset_symbol = AssetSymbol::new(&request.asset)
        .map_err(|_| ApiError::bad_request("INVALID_ASSET_SYMBOL", "invalid asset symbol"))?;

    let asset_decimals = {
        let exchange = state.exchange.lock().await;

        exchange
            .asset(&asset_symbol)
            .ok_or_else(|| ApiError::not_found("ASSET_NOT_FOUND", "asset not found"))?
            .decimals()
    };

    let amount_atomic = parse_decimal(&request.amount, asset_decimals)
        .map_err(|_| ApiError::bad_request("INVALID_AMOUNT", "invalid deposit amount"))?;

    if amount_atomic == 0 {
        return Err(ApiError::bad_request(
            "INVALID_AMOUNT",
            "deposit amount must be greater than zero",
        ));
    }

    let amount = AssetAmount::new(amount_atomic);

    let record = state
        .admin_service
        .deposit(&request.reference_id, user_id, &asset_symbol, amount)
        .await
        .map_err(ApiError::from)?;

    let response = DepositAssetResponse {
        id: record.id.to_string(),
        reference_id: record.reference_id,
        user_id: record.user_id,
        asset: asset_symbol.as_str().to_string(),
        amount: format_response_decimal(amount.value(), asset_decimals)?,
        status: record.status,
    };

    Ok((StatusCode::OK, Json(response)))
}
