use chrono::Utc;
use sqlx::{PgConnection, PgPool, types::BigDecimal};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    accounting::ledger::BalanceMovementReason,
    application::balance_mapping::{delta_to_big_decimal, movement_type},
    domain::{
        order::{OrderStatus, Side},
        pair::TradingPair,
        primitives::{OrderId, Price, Quantity, UserId},
    },
    exchange::{
        Exchange, ExchangeError, MarketOrderRequest, OrderCancellationResult, OrderPlacementResult,
    },
    persistence::postgres::{
        assets,
        balance_movements::{self, BalanceMovementInsert},
        balances,
        markets::{self, MarketStateUpdate},
        orders::{self, OrderInsert, OrderStateUpdate},
        trades::{self, TradeInsert},
        users,
    },
};

#[derive(Clone)]
pub struct TradingService {
    exchange: Arc<Mutex<Exchange>>,
    db: PgPool,
}

#[derive(Debug)]
pub enum TradingServiceError {
    Exchange(ExchangeError),
    Database(sqlx::Error),
    UnknownUser,
    UserDisabled,
    UnknownBaseAsset,
    UnknownQuoteAsset,
    UnknownMarket,
    MarketDisabled,
    IdentifierOutOfRange,
    UnexpectedEngineOutput,
    BaseAssetDisabled,
    QuoteAssetDisabled,
}

fn order_status_name(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::PendingTrigger => "PENDING_TRIGGER",
        OrderStatus::Open => "OPEN",
        OrderStatus::PartiallyFilled => "PARTIALLY_FILLED",
        OrderStatus::Filled => "FILLED",
        OrderStatus::Cancelled => "CANCELLED",
        OrderStatus::Expired => "EXPIRED",
    }
}

impl TradingService {
    pub fn new(exchange: Arc<Mutex<Exchange>>, db: PgPool) -> Self {
        Self { exchange, db }
    }

    async fn persist_placement_result(
        connection: &mut PgConnection,
        result: &OrderPlacementResult,
        pair: &TradingPair,
        base_asset_id: i64,
        quote_asset_id: i64,
        market_id: i64,
    ) -> Result<(), TradingServiceError> {
        for change in result
            .order_changes()
            .iter()
            .filter(|change| change.order_id() != result.order_id())
        {
            let order_id = i64::try_from(change.order_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let sequence_number = change
                .sequence()
                .map(|sequence| i64::try_from(sequence.value()))
                .transpose()
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let order_update = OrderStateUpdate {
                id: order_id,
                status: order_status_name(change.status()).to_string(),
                remaining_quantity_atomic: BigDecimal::from(change.remaining_quantity().value()),
                sequence_number,
            };

            orders::update(&mut *connection, &order_update)
                .await
                .map_err(TradingServiceError::Database)?;
        }

        let mut trade_ids_by_sequence: HashMap<u64, i64> = HashMap::new();

        for trade in result.trades() {
            let sequence = trade
                .sequence()
                .ok_or(TradingServiceError::UnexpectedEngineOutput)?;
            let database_trade_sequence = i64::try_from(sequence.value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;
            let maker_order_id = i64::try_from(trade.maker_order_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let taker_order_id = i64::try_from(trade.taker_order_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let maker_user_id = i64::try_from(trade.maker_user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let taker_user_id = i64::try_from(trade.taker_user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let taker_side = match trade.taker_side() {
                Side::Buy => "BUY",
                Side::Sell => "SELL",
            };

            let trade_insert = TradeInsert {
                market_id: market_id,
                trade_sequence: database_trade_sequence,
                maker_order_id,
                taker_order_id,
                maker_user_id,
                taker_user_id,
                taker_side: taker_side.to_string(),
                price_atomic: BigDecimal::from(trade.price().value()),
                quantity_atomic: BigDecimal::from(trade.quantity().value()),
            };
            let trade_record = trades::insert(&mut *connection, &trade_insert)
                .await
                .map_err(TradingServiceError::Database)?;

            trade_ids_by_sequence.insert(sequence.value(), trade_record.id);
        }

        let operation_id = Uuid::new_v4();

        for movement in result.balance_movements() {
            let movement_user_id = i64::try_from(movement.user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let asset_id = if movement.asset() == pair.base() {
                base_asset_id
            } else if movement.asset() == pair.quote() {
                quote_asset_id
            } else {
                return Err(TradingServiceError::UnexpectedEngineOutput);
            };

            let (order_id, trade_id) = match movement.reason() {
                BalanceMovementReason::OrderLock { order_id }
                | BalanceMovementReason::OrderUnlock { order_id } => {
                    let database_order_id = i64::try_from(order_id.value())
                        .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

                    (Some(database_order_id), None)
                }

                BalanceMovementReason::TradeSettlement {
                    order_id,
                    trade_sequence,
                } => {
                    let database_order_id = i64::try_from(order_id.value())
                        .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

                    let database_trade_id = trade_ids_by_sequence
                        .get(&trade_sequence.value())
                        .copied()
                        .ok_or(TradingServiceError::UnexpectedEngineOutput)?;

                    (Some(database_order_id), Some(database_trade_id))
                }

                BalanceMovementReason::Deposit | BalanceMovementReason::AdminAdjustment => {
                    return Err(TradingServiceError::UnexpectedEngineOutput);
                }
            };

            let movement_insert = BalanceMovementInsert {
                operation_id,
                user_id: movement_user_id,
                asset_id,
                available_delta_atomic: delta_to_big_decimal(movement.available_delta()),
                locked_delta_atomic: delta_to_big_decimal(movement.locked_delta()),
                movement_type: movement_type(movement.reason()).to_string(),
                order_id,
                trade_id,
            };

            balance_movements::insert(&mut *connection, &movement_insert)
                .await
                .map_err(TradingServiceError::Database)?;
        }

        for snapshot in result.balance_snapshots() {
            let snapshot_user_id = i64::try_from(snapshot.user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let asset_id = if snapshot.asset() == pair.base() {
                base_asset_id
            } else if snapshot.asset() == pair.quote() {
                quote_asset_id
            } else {
                return Err(TradingServiceError::UnexpectedEngineOutput);
            };

            balances::upsert(
                &mut *connection,
                snapshot_user_id,
                asset_id,
                snapshot.available(),
                snapshot.locked(),
            )
            .await
            .map_err(TradingServiceError::Database)?;
        }

        let market_snapshot = result.market_snapshot();

        if market_snapshot.pair() != pair {
            return Err(TradingServiceError::UnexpectedEngineOutput);
        }

        let next_order_sequence = i64::try_from(market_snapshot.next_order_sequence().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let next_trade_sequence = i64::try_from(market_snapshot.next_trade_sequence().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let market_update = MarketStateUpdate {
            id: market_id,
            last_trade_price_atomic: market_snapshot
                .last_trade_price()
                .map(|price| BigDecimal::from(price.value())),
            next_order_sequence,
            next_trade_sequence,
        };

        markets::update(&mut *connection, &market_update)
            .await
            .map_err(TradingServiceError::Database)?;

        Ok(())
    }

    pub async fn place_limit_order(
        &self,
        client_order_id: String,
        user_id: UserId,
        pair: TradingPair,
        side: Side,
        quantity: Quantity,
        price: Price,
    ) -> Result<OrderPlacementResult, TradingServiceError> {
        let database_user_id = i64::try_from(user_id.value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(TradingServiceError::Database)?;

        let user = users::find_by_id(transaction.as_mut(), database_user_id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownUser)?;

        if !user.enabled {
            return Err(TradingServiceError::UserDisabled);
        }

        let base_asset = assets::find_by_symbol(transaction.as_mut(), pair.base())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownBaseAsset)?;

        if !base_asset.enabled {
            return Err(TradingServiceError::BaseAssetDisabled);
        }

        let quote_asset = assets::find_by_symbol(transaction.as_mut(), pair.quote())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownQuoteAsset)?;

        if !quote_asset.enabled {
            return Err(TradingServiceError::QuoteAssetDisabled);
        }

        let market = markets::find_by_assets(transaction.as_mut(), base_asset.id, quote_asset.id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownMarket)?;

        if !market.enabled {
            return Err(TradingServiceError::MarketDisabled);
        }

        let result = staged_exchange
            .place_limit_order(user_id, &pair, side, price, quantity)
            .map_err(TradingServiceError::Exchange)?;

        let incoming_change = result
            .order_changes()
            .iter()
            .find(|change| change.order_id() == result.order_id())
            .ok_or(TradingServiceError::UnexpectedEngineOutput)?;
        let database_order_id = i64::try_from(result.order_id().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let sequence_number = incoming_change
            .sequence()
            .map(|sequence| i64::try_from(sequence.value()))
            .transpose()
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let side = match side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let status = order_status_name(incoming_change.status());

        let closed_at = match incoming_change.status() {
            OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Expired => Some(Utc::now()),
            _ => None,
        };

        let order_insert = OrderInsert {
            id: database_order_id,
            client_order_id,
            market_id: market.id,
            user_id: database_user_id,
            side: side.to_string(),
            status: status.to_string(),
            kind: "LIMIT".to_string(),
            original_quantity_atomic: BigDecimal::from(quantity.value()),
            remaining_quantity_atomic: BigDecimal::from(
                incoming_change.remaining_quantity().value(),
            ),
            limit_price_atomic: Some(BigDecimal::from(price.value())),
            stop_price_atomic: None,
            max_quote_amount_atomic: None,
            sequence_number,
            triggered_at: None,
            closed_at,
        };

        orders::insert(transaction.as_mut(), &order_insert)
            .await
            .map_err(TradingServiceError::Database)?;

        Self::persist_placement_result(
            transaction.as_mut(),
            &result,
            &pair,
            base_asset.id,
            quote_asset.id,
            market.id,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(TradingServiceError::Database)?;
        *live_exchange = staged_exchange;
        Ok(result)
    }

    pub async fn place_market_order(
        &self,
        client_order_id: String,
        user_id: UserId,
        pair: TradingPair,
        request: MarketOrderRequest,
    ) -> Result<OrderPlacementResult, TradingServiceError> {
        let database_user_id = i64::try_from(user_id.value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(TradingServiceError::Database)?;

        let user = users::find_by_id(transaction.as_mut(), database_user_id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownUser)?;

        if !user.enabled {
            return Err(TradingServiceError::UserDisabled);
        }

        let base_asset = assets::find_by_symbol(transaction.as_mut(), pair.base())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownBaseAsset)?;

        if !base_asset.enabled {
            return Err(TradingServiceError::BaseAssetDisabled);
        }

        let quote_asset = assets::find_by_symbol(transaction.as_mut(), pair.quote())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownQuoteAsset)?;

        if !quote_asset.enabled {
            return Err(TradingServiceError::QuoteAssetDisabled);
        }

        let market = markets::find_by_assets(transaction.as_mut(), base_asset.id, quote_asset.id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownMarket)?;

        if !market.enabled {
            return Err(TradingServiceError::MarketDisabled);
        }

        let result = staged_exchange
            .place_market_order(user_id, &pair, request)
            .map_err(TradingServiceError::Exchange)?;

        let (side, quantity, max_quote_amount_atomic) = match request {
            MarketOrderRequest::Buy {
                quantity,
                max_quote_amount,
            } => (
                Side::Buy,
                quantity,
                Some(BigDecimal::from(max_quote_amount.value())),
            ),

            MarketOrderRequest::Sell { quantity } => (Side::Sell, quantity, None),
        };

        let incoming_change = result
            .order_changes()
            .iter()
            .find(|change| change.order_id() == result.order_id())
            .ok_or(TradingServiceError::UnexpectedEngineOutput)?;

        if !matches!(
            incoming_change.status(),
            OrderStatus::Filled | OrderStatus::Expired
        ) {
            return Err(TradingServiceError::UnexpectedEngineOutput);
        }

        let side_name = match side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let database_order_id = i64::try_from(result.order_id().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let sequence_number = incoming_change
            .sequence()
            .map(|sequence| i64::try_from(sequence.value()))
            .transpose()
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let order_insert = OrderInsert {
            id: database_order_id,
            client_order_id,
            market_id: market.id,
            user_id: database_user_id,
            side: side_name.to_string(),
            status: order_status_name(incoming_change.status()).to_string(),
            kind: "MARKET".to_string(),
            original_quantity_atomic: BigDecimal::from(quantity.value()),
            remaining_quantity_atomic: BigDecimal::from(
                incoming_change.remaining_quantity().value(),
            ),
            limit_price_atomic: None,
            stop_price_atomic: None,
            max_quote_amount_atomic,
            sequence_number,
            triggered_at: None,
            closed_at: Some(Utc::now()),
        };

        orders::insert(transaction.as_mut(), &order_insert)
            .await
            .map_err(TradingServiceError::Database)?;

        Self::persist_placement_result(
            transaction.as_mut(),
            &result,
            &pair,
            base_asset.id,
            quote_asset.id,
            market.id,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(TradingServiceError::Database)?;
        *live_exchange = staged_exchange;
        Ok(result)
    }

    pub async fn place_stop_limit_order(
        &self,
        client_order_id: String,
        user_id: UserId,
        pair: TradingPair,
        side: Side,
        quantity: Quantity,
        stop_price: Price,
        limit_price: Price,
    ) -> Result<OrderPlacementResult, TradingServiceError> {
        let database_user_id = i64::try_from(user_id.value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(TradingServiceError::Database)?;

        let user = users::find_by_id(transaction.as_mut(), database_user_id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownUser)?;

        if !user.enabled {
            return Err(TradingServiceError::UserDisabled);
        }

        let base_asset = assets::find_by_symbol(transaction.as_mut(), pair.base())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownBaseAsset)?;

        if !base_asset.enabled {
            return Err(TradingServiceError::BaseAssetDisabled);
        }

        let quote_asset = assets::find_by_symbol(transaction.as_mut(), pair.quote())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownQuoteAsset)?;

        if !quote_asset.enabled {
            return Err(TradingServiceError::QuoteAssetDisabled);
        }

        let market = markets::find_by_assets(transaction.as_mut(), base_asset.id, quote_asset.id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownMarket)?;

        if !market.enabled {
            return Err(TradingServiceError::MarketDisabled);
        }

        let result = staged_exchange
            .place_stop_limit_order(user_id, &pair, side, stop_price, limit_price, quantity)
            .map_err(TradingServiceError::Exchange)?;

        let incoming_change = result
            .order_changes()
            .iter()
            .find(|change| change.order_id() == result.order_id())
            .ok_or(TradingServiceError::UnexpectedEngineOutput)?;

        if incoming_change.status() != OrderStatus::PendingTrigger
            || incoming_change.sequence().is_some()
            || incoming_change.remaining_quantity() != quantity
            || !result.trades().is_empty()
        {
            return Err(TradingServiceError::UnexpectedEngineOutput);
        }

        let side_name = match side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };

        let database_order_id = i64::try_from(result.order_id().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let order_insert = OrderInsert {
            id: database_order_id,
            client_order_id,
            market_id: market.id,
            user_id: database_user_id,
            side: side_name.to_string(),
            status: "PENDING_TRIGGER".to_string(),
            kind: "STOP_LIMIT".to_string(),
            original_quantity_atomic: BigDecimal::from(quantity.value()),
            remaining_quantity_atomic: BigDecimal::from(quantity.value()),
            limit_price_atomic: Some(BigDecimal::from(limit_price.value())),
            stop_price_atomic: Some(BigDecimal::from(stop_price.value())),
            max_quote_amount_atomic: None,
            sequence_number: None,
            triggered_at: None,
            closed_at: None,
        };

        orders::insert(transaction.as_mut(), &order_insert)
            .await
            .map_err(TradingServiceError::Database)?;

        Self::persist_placement_result(
            transaction.as_mut(),
            &result,
            &pair,
            base_asset.id,
            quote_asset.id,
            market.id,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(TradingServiceError::Database)?;
        *live_exchange = staged_exchange;
        Ok(result)
    }

    pub async fn cancel_order(
        &self,
        user_id: UserId,
        pair: TradingPair,
        order_id: OrderId,
    ) -> Result<OrderCancellationResult, TradingServiceError> {
        let database_user_id = i64::try_from(user_id.value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(TradingServiceError::Database)?;

        let user = users::find_by_id(transaction.as_mut(), database_user_id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownUser)?;

        if !user.enabled {
            return Err(TradingServiceError::UserDisabled);
        }

        let base_asset = assets::find_by_symbol(transaction.as_mut(), pair.base())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownBaseAsset)?;

        if !base_asset.enabled {
            return Err(TradingServiceError::BaseAssetDisabled);
        }

        let quote_asset = assets::find_by_symbol(transaction.as_mut(), pair.quote())
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownQuoteAsset)?;

        if !quote_asset.enabled {
            return Err(TradingServiceError::QuoteAssetDisabled);
        }

        let market = markets::find_by_assets(transaction.as_mut(), base_asset.id, quote_asset.id)
            .await
            .map_err(TradingServiceError::Database)?
            .ok_or(TradingServiceError::UnknownMarket)?;

        if !market.enabled {
            return Err(TradingServiceError::MarketDisabled);
        }

        let result = staged_exchange
            .cancel_order(user_id, &pair, order_id)
            .map_err(TradingServiceError::Exchange)?;

        let change = result.order_change();

        if change.order_id() != order_id || change.status() != OrderStatus::Cancelled {
            return Err(TradingServiceError::UnexpectedEngineOutput);
        }

        let database_order_id = i64::try_from(change.order_id().value())
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let sequence_number = change
            .sequence()
            .map(|sequence| i64::try_from(sequence.value()))
            .transpose()
            .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

        let order_update = OrderStateUpdate {
            id: database_order_id,
            status: order_status_name(change.status()).to_string(),
            remaining_quantity_atomic: BigDecimal::from(change.remaining_quantity().value()),
            sequence_number,
        };

        orders::update(transaction.as_mut(), &order_update)
            .await
            .map_err(TradingServiceError::Database)?;

        let operation_id = Uuid::new_v4();

        for movement in result.balance_movements() {
            let movement_user_id = i64::try_from(movement.user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let asset_id = if movement.asset() == pair.base() {
                base_asset.id
            } else if movement.asset() == pair.quote() {
                quote_asset.id
            } else {
                return Err(TradingServiceError::UnexpectedEngineOutput);
            };

            let movement_order_id = match movement.reason() {
                BalanceMovementReason::OrderUnlock { order_id } => i64::try_from(order_id.value())
                    .map_err(|_| TradingServiceError::IdentifierOutOfRange)?,

                _ => return Err(TradingServiceError::UnexpectedEngineOutput),
            };

            if movement_order_id != database_order_id {
                return Err(TradingServiceError::UnexpectedEngineOutput);
            }

            let movement_insert = BalanceMovementInsert {
                operation_id,
                user_id: movement_user_id,
                asset_id,
                available_delta_atomic: delta_to_big_decimal(movement.available_delta()),
                locked_delta_atomic: delta_to_big_decimal(movement.locked_delta()),
                movement_type: movement_type(movement.reason()).to_string(),
                order_id: Some(movement_order_id),
                trade_id: None,
            };

            balance_movements::insert(transaction.as_mut(), &movement_insert)
                .await
                .map_err(TradingServiceError::Database)?;
        }

        for snapshot in result.balance_snapshots() {
            let snapshot_user_id = i64::try_from(snapshot.user_id().value())
                .map_err(|_| TradingServiceError::IdentifierOutOfRange)?;

            let asset_id = if snapshot.asset() == pair.base() {
                base_asset.id
            } else if snapshot.asset() == pair.quote() {
                quote_asset.id
            } else {
                return Err(TradingServiceError::UnexpectedEngineOutput);
            };

            balances::upsert(
                transaction.as_mut(),
                snapshot_user_id,
                asset_id,
                snapshot.available(),
                snapshot.locked(),
            )
            .await
            .map_err(TradingServiceError::Database)?;
        }

        transaction
            .commit()
            .await
            .map_err(TradingServiceError::Database)?;

        *live_exchange = staged_exchange;
        Ok(result)
    }
}
