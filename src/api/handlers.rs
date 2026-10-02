use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use num_traits::ToPrimitive;
use sqlx::{FromRow, types::BigDecimal};
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
        AccountBalancesResponse, AccountOrderResponse, AccountOrdersResponse, AccountTradeResponse,
        AccountTradesResponse, AssetResponse, BalanceResponse, BookTickerResponse,
        CancelOrderResponse, CandleResponse, CandlesQuery, CandlesResponse, CreateAssetRequest,
        CreateMarketRequest, CreateUserRequest, DepositAssetRequest, DepositAssetResponse,
        DepthLevelResponse, MarketDepthQuery, MarketDepthResponse, MarketResponse,
        MarketStatsResponse, MarketSummaryResponse, MarketsResponse, OpenOrderResponse,
        OpenOrdersResponse, OrderPlacementResponse, PlaceLimitOrderRequest,
        PlaceMarketOrderRequest, PlaceStopLimitOrderRequest, PlaceStopLimitOrderResponse,
        RecentTradeResponse, RecentTradesQuery, RecentTradesResponse, TradeResponse, UserResponse,
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

    let result = state
        .trading_service
        .cancel_order(user_id, pair, order_id)
        .await
        .map_err(ApiError::from)?;
    let cancelled_order = result.cancelled_order();

    let price = cancelled_order.limit_price().ok_or_else(|| {
        ApiError::internal(
            "ORDER_CONFIGURATION_ERROR",
            "cancelled limit order has no price",
        )
    })?;

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

    let result = state
        .trading_service
        .place_market_order(client_order_id.clone(), user_id, pair, market_request)
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

    let result = state
        .trading_service
        .place_stop_limit_order(
            client_order_id.clone(),
            user_id,
            pair,
            side,
            quantity,
            stop_price,
            limit_price,
        )
        .await
        .map_err(ApiError::from)?;

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

fn stored_atomic_to_u128(value: &BigDecimal) -> Result<u128, ApiError> {
    value.to_u128().ok_or_else(|| {
        ApiError::internal("INVALID_STORED_AMOUNT", "stored atomic amount is invalid")
    })
}

pub async fn list_markets(
    State(state): State<AppState>,
) -> Result<Json<MarketsResponse>, ApiError> {
    let records = state
        .market_data_service
        .list_markets()
        .await
        .map_err(ApiError::from)?;
    let mut markets = Vec::with_capacity(records.len());

    for record in records {
        let base_decimals = u8::try_from(record.base_decimals).map_err(|_| {
            ApiError::internal(
                "INVALID_STORED_MARKET",
                "stored base asset decimals are invalid",
            )
        })?;

        let quote_decimals = u8::try_from(record.quote_decimals).map_err(|_| {
            ApiError::internal(
                "INVALID_STORED_MARKET",
                "stored quote asset decimals are invalid",
            )
        })?;

        let price_tick = format_response_decimal(
            stored_atomic_to_u128(&record.price_tick_atomic)?,
            quote_decimals,
        )?;

        let quantity_step = format_response_decimal(
            stored_atomic_to_u128(&record.quantity_step_atomic)?,
            base_decimals,
        )?;

        let last_trade_price = match &record.last_trade_price_atomic {
            Some(value) => Some(format_response_decimal(
                stored_atomic_to_u128(value)?,
                quote_decimals,
            )?),
            None => None,
        };

        markets.push(MarketSummaryResponse {
            id: record.id,
            base: record.base_symbol,
            quote: record.quote_symbol,
            price_tick,
            quantity_step,
            last_trade_price,
            enabled: record.enabled,
        });
    }

    Ok(Json(MarketsResponse { markets }))
}

pub async fn market_depth(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
    Query(query): Query<MarketDepthQuery>,
) -> Result<Json<MarketDepthResponse>, ApiError> {
    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let limit = query.limit.unwrap_or(20);

    if !(1..=100).contains(&limit) {
        return Err(ApiError::bad_request(
            "INVALID_DEPTH_LIMIT",
            "depth limit must be between 1 and 100",
        ));
    }

    let (depth, base_decimals, quote_decimals) = {
        let exchange = state.exchange.lock().await;

        let base_decimals = exchange
            .asset(pair.base())
            .ok_or_else(|| ApiError::not_found("ASSET_NOT_FOUND", "base asset not found"))?
            .decimals();

        let quote_decimals = exchange
            .asset(pair.quote())
            .ok_or_else(|| ApiError::not_found("ASSET_NOT_FOUND", "quote asset not found"))?
            .decimals();

        let depth = exchange
            .market_depth(&pair, limit)
            .map_err(ApiError::from)?;

        (depth, base_decimals, quote_decimals)
    };

    let mut bids = Vec::with_capacity(depth.bids().len());

    for level in depth.bids() {
        bids.push(DepthLevelResponse {
            price: format_response_decimal(level.price().value(), quote_decimals)?,
            quantity: format_response_decimal(level.quantity().value(), base_decimals)?,
        });
    }

    let mut asks = Vec::with_capacity(depth.asks().len());

    for level in depth.asks() {
        asks.push(DepthLevelResponse {
            price: format_response_decimal(level.price().value(), quote_decimals)?,
            quantity: format_response_decimal(level.quantity().value(), base_decimals)?,
        });
    }

    Ok(Json(MarketDepthResponse {
        base: pair.base().as_str().to_string(),
        quote: pair.quote().as_str().to_string(),
        bids,
        asks,
    }))
}

pub async fn recent_trades(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
    Query(query): Query<RecentTradesQuery>,
) -> Result<Json<RecentTradesResponse>, ApiError> {
    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let limit = query.limit.unwrap_or(50);

    if !(1..=100).contains(&limit) {
        return Err(ApiError::bad_request(
            "INVALID_TRADES_LIMIT",
            "recent trades limit must be between 1 and 100",
        ));
    }

    let result = state
        .market_data_service
        .recent_trades(&pair, limit)
        .await
        .map_err(ApiError::from)?;

    let (market, records) = result.into_parts();

    let base_decimals = u8::try_from(market.base_decimals).map_err(|_| {
        ApiError::internal(
            "INVALID_STORED_MARKET",
            "stored base asset decimals are invalid",
        )
    })?;

    let quote_decimals = u8::try_from(market.quote_decimals).map_err(|_| {
        ApiError::internal(
            "INVALID_STORED_MARKET",
            "stored quote asset decimals are invalid",
        )
    })?;

    let mut trades = Vec::with_capacity(records.len());

    for record in records {
        trades.push(RecentTradeResponse {
            trade_id: record.id.to_string(),
            sequence: record.trade_sequence.to_string(),
            price: format_response_decimal(
                stored_atomic_to_u128(&record.price_atomic)?,
                quote_decimals,
            )?,
            quantity: format_response_decimal(
                stored_atomic_to_u128(&record.quantity_atomic)?,
                base_decimals,
            )?,
            taker_side: record.taker_side,
            executed_at: record.created_at.to_rfc3339(),
        });
    }

    Ok(Json(RecentTradesResponse {
        base: market.base_symbol,
        quote: market.quote_symbol,
        trades,
    }))
}

pub async fn get_candles(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
    Query(query): Query<CandlesQuery>,
) -> Result<Json<CandlesResponse>, ApiError> {
    let interval = query.interval.unwrap_or_else(|| "1m".to_string());

    if interval != "1m" {
        return Err(ApiError::bad_request(
            "UNSUPPORTED_CANDLE_INTERVAL",
            "only the 1m candle interval is currently supported",
        ));
    }

    let limit = query.limit.unwrap_or(100);

    let base_symbol = AssetSymbol::new(&base)
        .map_err(|_| ApiError::bad_request("INVALID_BASE_SYMBOL", "invalid base symbol"))?;

    let quote_symbol = AssetSymbol::new(&quote)
        .map_err(|_| ApiError::bad_request("INVALID_QUOTE_SYMBOL", "invalid quote symbol"))?;

    let pair = TradingPair::new(base_symbol, quote_symbol)
        .map_err(|_| ApiError::bad_request("INVALID_TRADING_PAIR", "invalid trading pair"))?;

    let result = state
        .market_data_service
        .minute_candles(&pair, limit)
        .await
        .map_err(ApiError::from)?;

    let (market, candle_records) = result.into_parts();

    let base_decimals = u8::try_from(market.base_decimals).map_err(|_| {
        ApiError::internal(
            "INVALID_STORED_MARKET",
            "stored base asset decimals are invalid",
        )
    })?;

    let quote_decimals = u8::try_from(market.quote_decimals).map_err(|_| {
        ApiError::internal(
            "INVALID_STORED_MARKET",
            "stored quote asset decimals are invalid",
        )
    })?;

    let mut candles = Vec::with_capacity(candle_records.len());

    for candle in candle_records {
        candles.push(CandleResponse {
            time: candle.start_time().timestamp(),
            open: format_response_decimal(candle.open().value(), quote_decimals)?,
            high: format_response_decimal(candle.high().value(), quote_decimals)?,
            low: format_response_decimal(candle.low().value(), quote_decimals)?,
            close: format_response_decimal(candle.close().value(), quote_decimals)?,
            volume: format_response_decimal(candle.volume().value(), base_decimals)?,
        });
    }

    Ok(Json(CandlesResponse {
        base: market.base_symbol,
        quote: market.quote_symbol,
        interval,
        candles,
    }))
}

#[derive(FromRow)]
struct OpenOrderRecord {
    id: i64,
    side: String,
    kind: String,
    status: String,
    remaining_quantity_atomic: BigDecimal,
    limit_price_atomic: BigDecimal,
    base_decimals: i16,
    quote_decimals: i16,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn open_orders(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<OpenOrdersResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;
    let user_id = i64::try_from(user_id.value())
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id is invalid"))?;
    let records = sqlx::query_as::<_, OpenOrderRecord>(
        r#"
        SELECT o.id, o.side, o.kind, o.status, o.remaining_quantity_atomic,
               o.limit_price_atomic, b.decimals AS base_decimals,
               q.decimals AS quote_decimals, o.created_at
        FROM orders o
        JOIN markets m ON m.id = o.market_id
        JOIN assets b ON b.id = m.base_asset_id
        JOIN assets q ON q.id = m.quote_asset_id
        WHERE o.user_id = $1 AND b.symbol = $2 AND q.symbol = $3
          AND o.status IN ('PENDING_TRIGGER', 'OPEN', 'PARTIALLY_FILLED')
        ORDER BY o.created_at DESC, o.id DESC
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .bind(base)
    .bind(quote)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::internal("ORDERS_UNAVAILABLE", "open orders could not be loaded"))?;

    let mut orders = Vec::with_capacity(records.len());
    for record in records {
        let base_decimals = u8::try_from(record.base_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_ORDER", "invalid base precision"))?;
        let quote_decimals = u8::try_from(record.quote_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_ORDER", "invalid quote precision"))?;
        orders.push(OpenOrderResponse {
            order_id: record.id.to_string(),
            side: record.side.to_ascii_lowercase(),
            kind: record.kind.to_ascii_lowercase().replace('_', "-"),
            status: record.status.to_ascii_lowercase().replace('_', "-"),
            remaining_quantity: format_response_decimal(
                stored_atomic_to_u128(&record.remaining_quantity_atomic)?,
                base_decimals,
            )?,
            price: format_response_decimal(
                stored_atomic_to_u128(&record.limit_price_atomic)?,
                quote_decimals,
            )?,
            created_at: record.created_at.to_rfc3339(),
        });
    }
    Ok(Json(OpenOrdersResponse { orders }))
}

#[derive(FromRow)]
struct AccountOrderRecord {
    id: i64,
    base: String,
    quote: String,
    side: String,
    kind: String,
    status: String,
    original_quantity_atomic: BigDecimal,
    remaining_quantity_atomic: BigDecimal,
    limit_price_atomic: Option<BigDecimal>,
    base_decimals: i16,
    quote_decimals: i16,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn account_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AccountOrdersResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;
    let user_id = i64::try_from(user_id.value())
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id is invalid"))?;
    let records = sqlx::query_as::<_, AccountOrderRecord>(
        r#"
        SELECT o.id, b.symbol AS base, q.symbol AS quote, o.side, o.kind, o.status,
               o.original_quantity_atomic, o.remaining_quantity_atomic, o.limit_price_atomic,
               b.decimals AS base_decimals, q.decimals AS quote_decimals, o.created_at
        FROM orders o
        JOIN markets m ON m.id = o.market_id
        JOIN assets b ON b.id = m.base_asset_id
        JOIN assets q ON q.id = m.quote_asset_id
        WHERE o.user_id = $1
        ORDER BY o.created_at DESC, o.id DESC
        LIMIT 200
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::internal("ORDERS_UNAVAILABLE", "account orders could not be loaded"))?;
    let mut orders = Vec::with_capacity(records.len());
    for record in records {
        let base_decimals = u8::try_from(record.base_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_ORDER", "invalid base precision"))?;
        let quote_decimals = u8::try_from(record.quote_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_ORDER", "invalid quote precision"))?;
        orders.push(AccountOrderResponse {
            order_id: record.id.to_string(),
            base: record.base,
            quote: record.quote,
            side: record.side.to_ascii_lowercase(),
            kind: record.kind.to_ascii_lowercase().replace('_', "-"),
            status: record.status.to_ascii_lowercase().replace('_', "-"),
            original_quantity: format_response_decimal(
                stored_atomic_to_u128(&record.original_quantity_atomic)?,
                base_decimals,
            )?,
            remaining_quantity: format_response_decimal(
                stored_atomic_to_u128(&record.remaining_quantity_atomic)?,
                base_decimals,
            )?,
            price: record
                .limit_price_atomic
                .as_ref()
                .map(|value| format_response_decimal(stored_atomic_to_u128(value)?, quote_decimals))
                .transpose()?,
            created_at: record.created_at.to_rfc3339(),
        });
    }
    Ok(Json(AccountOrdersResponse { orders }))
}

#[derive(FromRow)]
struct AccountTradeRecord {
    id: i64,
    base: String,
    quote: String,
    side: String,
    price_atomic: BigDecimal,
    quantity_atomic: BigDecimal,
    base_decimals: i16,
    quote_decimals: i16,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn account_trades(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AccountTradesResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;
    let user_id = i64::try_from(user_id.value())
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id is invalid"))?;
    let records = sqlx::query_as::<_, AccountTradeRecord>(
        r#"
        SELECT t.id, b.symbol AS base, q.symbol AS quote,
               CASE WHEN t.taker_user_id = $1 THEN t.taker_side
                    WHEN t.taker_side = 'BUY' THEN 'SELL' ELSE 'BUY' END AS side,
               t.price_atomic, t.quantity_atomic,
               b.decimals AS base_decimals, q.decimals AS quote_decimals, t.created_at
        FROM trades t
        JOIN markets m ON m.id = t.market_id
        JOIN assets b ON b.id = m.base_asset_id
        JOIN assets q ON q.id = m.quote_asset_id
        WHERE t.maker_user_id = $1 OR t.taker_user_id = $1
        ORDER BY t.created_at DESC, t.id DESC
        LIMIT 200
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| ApiError::internal("TRADES_UNAVAILABLE", "account trades could not be loaded"))?;
    let mut trades = Vec::with_capacity(records.len());
    for record in records {
        let base_decimals = u8::try_from(record.base_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_TRADE", "invalid base precision"))?;
        let quote_decimals = u8::try_from(record.quote_decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_TRADE", "invalid quote precision"))?;
        trades.push(AccountTradeResponse {
            trade_id: record.id.to_string(),
            base: record.base,
            quote: record.quote,
            side: record.side.to_ascii_lowercase(),
            price: format_response_decimal(
                stored_atomic_to_u128(&record.price_atomic)?,
                quote_decimals,
            )?,
            quantity: format_response_decimal(
                stored_atomic_to_u128(&record.quantity_atomic)?,
                base_decimals,
            )?,
            executed_at: record.created_at.to_rfc3339(),
        });
    }
    Ok(Json(AccountTradesResponse { trades }))
}

#[derive(FromRow)]
struct AccountBalanceRecord {
    asset: String,
    decimals: i16,
    available_atomic: BigDecimal,
    locked_atomic: BigDecimal,
}

pub async fn account_balances(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AccountBalancesResponse>, ApiError> {
    let user_id = user_id_from_header(&headers)?;
    let user_id = i64::try_from(user_id.value())
        .map_err(|_| ApiError::unauthorized("INVALID_USER_ID", "X-User-Id is invalid"))?;
    let records = sqlx::query_as::<_, AccountBalanceRecord>(
        r#"
        SELECT a.symbol AS asset, a.decimals, b.available_atomic, b.locked_atomic
        FROM balances b JOIN assets a ON a.id = b.asset_id
        WHERE b.user_id = $1 ORDER BY a.symbol
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| {
        ApiError::internal(
            "BALANCES_UNAVAILABLE",
            "account balances could not be loaded",
        )
    })?;
    let mut balances = Vec::with_capacity(records.len());
    for record in records {
        let decimals = u8::try_from(record.decimals)
            .map_err(|_| ApiError::internal("INVALID_STORED_BALANCE", "invalid asset precision"))?;
        balances.push(BalanceResponse {
            asset: record.asset,
            available: format_response_decimal(
                stored_atomic_to_u128(&record.available_atomic)?,
                decimals,
            )?,
            locked: format_response_decimal(
                stored_atomic_to_u128(&record.locked_atomic)?,
                decimals,
            )?,
        });
    }
    Ok(Json(AccountBalancesResponse { balances }))
}

#[derive(FromRow)]
struct MarketStatsRecord {
    base_decimals: i16,
    quote_decimals: i16,
    last_price_atomic: Option<BigDecimal>,
    open_price_atomic: Option<BigDecimal>,
    high_price_atomic: Option<BigDecimal>,
    low_price_atomic: Option<BigDecimal>,
    volume_base_atomic: Option<BigDecimal>,
    volume_quote_atomic: Option<BigDecimal>,
}

pub async fn market_stats(
    State(state): State<AppState>,
    Path((base, quote)): Path<(String, String)>,
) -> Result<Json<MarketStatsResponse>, ApiError> {
    let record = sqlx::query_as::<_, MarketStatsRecord>(
        r#"
        SELECT b.decimals AS base_decimals, q.decimals AS quote_decimals,
               m.last_trade_price_atomic AS last_price_atomic,
               (SELECT t.price_atomic FROM trades t WHERE t.market_id = m.id AND t.created_at >= NOW() - INTERVAL '24 hours' ORDER BY t.created_at ASC, t.id ASC LIMIT 1) AS open_price_atomic,
               (SELECT MAX(t.price_atomic) FROM trades t WHERE t.market_id = m.id AND t.created_at >= NOW() - INTERVAL '24 hours') AS high_price_atomic,
               (SELECT MIN(t.price_atomic) FROM trades t WHERE t.market_id = m.id AND t.created_at >= NOW() - INTERVAL '24 hours') AS low_price_atomic,
               (SELECT SUM(t.quantity_atomic) FROM trades t WHERE t.market_id = m.id AND t.created_at >= NOW() - INTERVAL '24 hours') AS volume_base_atomic,
               (SELECT SUM(t.price_atomic * t.quantity_atomic / POWER(10::numeric, b.decimals::numeric)) FROM trades t WHERE t.market_id = m.id AND t.created_at >= NOW() - INTERVAL '24 hours') AS volume_quote_atomic
        FROM markets m
        JOIN assets b ON b.id = m.base_asset_id
        JOIN assets q ON q.id = m.quote_asset_id
        WHERE b.symbol = $1 AND q.symbol = $2
        "#,
    )
    .bind(&base)
    .bind(&quote)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| ApiError::internal("STATS_UNAVAILABLE", "market statistics could not be loaded"))?
    .ok_or_else(|| ApiError::not_found("MARKET_NOT_FOUND", "market not found"))?;
    let quote_decimals = u8::try_from(record.quote_decimals)
        .map_err(|_| ApiError::internal("INVALID_STORED_MARKET", "invalid quote precision"))?;
    let base_decimals = u8::try_from(record.base_decimals)
        .map_err(|_| ApiError::internal("INVALID_STORED_MARKET", "invalid base precision"))?;
    let format_price = |value: Option<BigDecimal>| -> Result<Option<String>, ApiError> {
        value
            .as_ref()
            .map(|amount| format_response_decimal(stored_atomic_to_u128(amount)?, quote_decimals))
            .transpose()
    };
    Ok(Json(MarketStatsResponse {
        base,
        quote,
        last_price: format_price(record.last_price_atomic)?,
        open_24h: format_price(record.open_price_atomic)?,
        high_24h: format_price(record.high_price_atomic)?,
        low_24h: format_price(record.low_price_atomic)?,
        volume_24h_base: format_response_decimal(
            record
                .volume_base_atomic
                .as_ref()
                .map(stored_atomic_to_u128)
                .transpose()?
                .unwrap_or(0),
            base_decimals,
        )?,
        volume_24h_quote: format_response_decimal(
            record
                .volume_quote_atomic
                .as_ref()
                .map(stored_atomic_to_u128)
                .transpose()?
                .unwrap_or(0),
            quote_decimals,
        )?,
    }))
}
