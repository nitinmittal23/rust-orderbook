use num_traits::ToPrimitive;
use sqlx::{PgPool, types::BigDecimal};
use std::collections::HashMap;

use crate::{
    domain::{
        asset::{Asset, AssetSymbol},
        order::{Order, OrderKind, Side},
        pair::TradingPair,
        primitives::{
            AssetAmount, OrderId, Price, Quantity, SequenceNumber, TradeSequenceNumber, UserId,
        },
        stop_order::StopLimitOrder,
    },
    exchange::Exchange,
    persistence::postgres::{assets, balances, markets, orders},
};

#[derive(Debug)]
pub enum ExchangeLoadError {
    Database(sqlx::Error),
    InvalidPersistedState(String),
}

fn atomic_to_u128(value: &BigDecimal, field: &str) -> Result<u128, ExchangeLoadError> {
    value.to_u128().ok_or_else(|| {
        ExchangeLoadError::InvalidPersistedState(format!(
            "{field} is negative, fractional, or too large"
        ))
    })
}

fn positive_i64_to_u64(value: i64, field: &str) -> Result<u64, ExchangeLoadError> {
    let value = u64::try_from(value)
        .map_err(|_| ExchangeLoadError::InvalidPersistedState(format!("{field} is negative")))?;

    if value == 0 {
        return Err(ExchangeLoadError::InvalidPersistedState(format!(
            "{field} must be greater than zero"
        )));
    }

    Ok(value)
}

pub async fn load_exchange(db: &PgPool) -> Result<Exchange, ExchangeLoadError> {
    let mut transaction = db.begin().await.map_err(ExchangeLoadError::Database)?;

    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let asset_rows = assets::list_all(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let market_rows = markets::list_all(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let balance_rows = balances::list_all(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let active_order_rows = orders::list_active(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let pending_stop_rows = orders::list_pending_stops(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let maximum_order_id = orders::max_id(transaction.as_mut())
        .await
        .map_err(ExchangeLoadError::Database)?;

    let mut exchange = Exchange::new();
    let mut asset_symbols_by_id: HashMap<i64, AssetSymbol> = HashMap::new();

    for row in asset_rows {
        positive_i64_to_u64(row.id, "asset id")?;

        let symbol = AssetSymbol::new(&row.symbol).map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "invalid asset symbol {}: {error:?}",
                row.symbol,
            ))
        })?;

        let decimals = u8::try_from(row.decimals).map_err(|_| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "invalid decimals for asset {}",
                row.symbol,
            ))
        })?;

        let asset = Asset::new(symbol.clone(), decimals).map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "invalid asset {}: {error:?}",
                row.symbol,
            ))
        })?;

        exchange.register_asset(asset).map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "could not restore asset {}: {error:?}",
                row.symbol,
            ))
        })?;

        if asset_symbols_by_id.insert(row.id, symbol).is_some() {
            return Err(ExchangeLoadError::InvalidPersistedState(format!(
                "duplicate asset id {}",
                row.id
            )));
        }
    }

    let mut market_pairs_by_id: HashMap<i64, TradingPair> = HashMap::new();

    for row in market_rows {
        positive_i64_to_u64(row.id, "market id")?;

        let base = asset_symbols_by_id
            .get(&row.base_asset_id)
            .cloned()
            .ok_or_else(|| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "market {} references unknown base asset {}",
                    row.id, row.base_asset_id,
                ))
            })?;

        let quote = asset_symbols_by_id
            .get(&row.quote_asset_id)
            .cloned()
            .ok_or_else(|| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "market {} references unknown quote asset {}",
                    row.id, row.quote_asset_id,
                ))
            })?;

        let pair = TradingPair::new(base, quote).map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "invalid market pair {}: {error:?}",
                row.id,
            ))
        })?;

        let price_tick = Price::new(atomic_to_u128(&row.price_tick_atomic, "market price tick")?)
            .map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "invalid price tick for market {}: {error:?}",
                row.id,
            ))
        })?;

        let quantity_step = Quantity::new(atomic_to_u128(
            &row.quantity_step_atomic,
            "market quantity step",
        )?);

        let last_trade_price = match &row.last_trade_price_atomic {
            Some(value) => Some(
                Price::new(atomic_to_u128(value, "last trade price")?).map_err(|error| {
                    ExchangeLoadError::InvalidPersistedState(format!(
                        "invalid last trade price for market {}: {error:?}",
                        row.id,
                    ))
                })?,
            ),
            None => None,
        };

        let next_order_sequence = SequenceNumber::new(positive_i64_to_u64(
            row.next_order_sequence,
            "next order sequence",
        )?);

        let next_trade_sequence = TradeSequenceNumber::new(positive_i64_to_u64(
            row.next_trade_sequence,
            "next trade sequence",
        )?);

        exchange
            .restore_market(
                pair.clone(),
                price_tick,
                quantity_step,
                last_trade_price,
                next_order_sequence,
                next_trade_sequence,
            )
            .map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "could not restore market {}: {error:?}",
                    row.id,
                ))
            })?;

        if market_pairs_by_id.insert(row.id, pair).is_some() {
            return Err(ExchangeLoadError::InvalidPersistedState(format!(
                "duplicate market id {}",
                row.id
            )));
        }
    }

    for row in balance_rows {
        let user_id = UserId::new(positive_i64_to_u64(row.user_id, "balance user id")?);

        let asset = asset_symbols_by_id.get(&row.asset_id).ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "balance references unknown asset {}",
                row.asset_id,
            ))
        })?;

        let available =
            AssetAmount::new(atomic_to_u128(&row.available_atomic, "available balance")?);

        let locked = AssetAmount::new(atomic_to_u128(&row.locked_atomic, "locked balance")?);

        exchange
            .restore_balance(user_id, asset, available, locked)
            .map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "could not restore balance for user {} and asset {}: {error:?}",
                    row.user_id, row.asset_id,
                ))
            })?;
    }

    for row in active_order_rows {
        let pair = market_pairs_by_id.get(&row.market_id).ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "order {} references unknown market {}",
                row.id, row.market_id,
            ))
        })?;

        let order_id = OrderId::new(positive_i64_to_u64(row.id, "order id")?);

        let user_id = UserId::new(positive_i64_to_u64(row.user_id, "order user id")?);

        let side = match row.side.as_str() {
            "BUY" => Side::Buy,
            "SELL" => Side::Sell,
            value => {
                return Err(ExchangeLoadError::InvalidPersistedState(format!(
                    "order {} has invalid side {value}",
                    row.id
                )));
            }
        };

        match row.kind.as_str() {
            "LIMIT" | "STOP_LIMIT" => {}
            value => {
                return Err(ExchangeLoadError::InvalidPersistedState(format!(
                    "active order {} has invalid kind {value}",
                    row.id,
                )));
            }
        }

        let limit_price_atomic = row.limit_price_atomic.as_ref().ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "active order {} has no limit price",
                row.id,
            ))
        })?;

        let limit_price = Price::new(atomic_to_u128(limit_price_atomic, "order limit price")?)
            .map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "order {} has invalid limit price: {error:?}",
                    row.id,
                ))
            })?;

        let original_quantity = Quantity::new(atomic_to_u128(
            &row.original_quantity_atomic,
            "order original quantity",
        )?);

        let remaining_quantity = Quantity::new(atomic_to_u128(
            &row.remaining_quantity_atomic,
            "order remaining quantity",
        )?);

        let sequence = row.sequence_number.ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "active order {} has no sequence",
                row.id,
            ))
        })?;

        let sequence = SequenceNumber::new(positive_i64_to_u64(sequence, "order sequence")?);

        let order = Order::restore_active(
            order_id,
            user_id,
            side,
            OrderKind::Limit { price: limit_price },
            original_quantity,
            remaining_quantity,
            sequence,
        )
        .map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "could not reconstruct order {}: {error:?}",
                row.id,
            ))
        })?;

        exchange
            .restore_active_order(pair, order)
            .map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "could not restore order {}: {error:?}",
                    row.id,
                ))
            })?;
    }

    for row in pending_stop_rows {
        let pair = market_pairs_by_id.get(&row.market_id).ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "pending stop {} references unknown market {}",
                row.id, row.market_id,
            ))
        })?;

        if row.kind != "STOP_LIMIT" {
            return Err(ExchangeLoadError::InvalidPersistedState(format!(
                "pending order {} has invalid kind {}",
                row.id, row.kind,
            )));
        }

        if row.sequence_number.is_some() {
            return Err(ExchangeLoadError::InvalidPersistedState(format!(
                "pending stop {} already has a sequence",
                row.id,
            )));
        }

        let side = match row.side.as_str() {
            "BUY" => Side::Buy,
            "SELL" => Side::Sell,
            value => {
                return Err(ExchangeLoadError::InvalidPersistedState(format!(
                    "pending stop {} has invalid side {value}",
                    row.id,
                )));
            }
        };

        let stop_price_atomic = row.stop_price_atomic.as_ref().ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "pending stop {} has no stop price",
                row.id,
            ))
        })?;

        let limit_price_atomic = row.limit_price_atomic.as_ref().ok_or_else(|| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "pending stop {} has no limit price",
                row.id,
            ))
        })?;

        let stop_price =
            Price::new(atomic_to_u128(stop_price_atomic, "stop price")?).map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "pending stop {} has invalid stop price: {error:?}",
                    row.id,
                ))
            })?;

        let limit_price = Price::new(atomic_to_u128(limit_price_atomic, "stop limit price")?)
            .map_err(|error| {
                ExchangeLoadError::InvalidPersistedState(format!(
                    "pending stop {} has invalid limit price: {error:?}",
                    row.id,
                ))
            })?;

        let original_quantity = atomic_to_u128(
            &row.original_quantity_atomic,
            "pending stop original quantity",
        )?;

        let remaining_quantity = atomic_to_u128(
            &row.remaining_quantity_atomic,
            "pending stop remaining quantity",
        )?;

        if remaining_quantity != original_quantity {
            return Err(ExchangeLoadError::InvalidPersistedState(format!(
                "pending stop {} has inconsistent quantity",
                row.id,
            )));
        }

        let stop = StopLimitOrder::new(
            OrderId::new(positive_i64_to_u64(row.id, "pending stop id")?),
            UserId::new(positive_i64_to_u64(row.user_id, "pending stop user id")?),
            side,
            stop_price,
            limit_price,
            Quantity::new(original_quantity),
        )
        .map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "could not reconstruct pending stop {}: {error:?}",
                row.id,
            ))
        })?;

        exchange.restore_pending_stop(pair, stop).map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "could not restore pending stop {}: {error:?}",
                row.id,
            ))
        })?;
    }

    let next_order_id = maximum_order_id.checked_add(1).ok_or_else(|| {
        ExchangeLoadError::InvalidPersistedState("next order id overflowed".to_string())
    })?;

    exchange
        .restore_next_order_id(positive_i64_to_u64(next_order_id, "next order id")?)
        .map_err(|error| {
            ExchangeLoadError::InvalidPersistedState(format!(
                "could not restore next order id: {error:?}",
            ))
        })?;

    transaction
        .commit()
        .await
        .map_err(ExchangeLoadError::Database)?;

    Ok(exchange)
}
