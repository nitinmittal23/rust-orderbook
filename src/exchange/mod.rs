use crate::accounting::ledger::{Ledger, LedgerError};
use crate::domain::{
    asset::{Asset, AssetSymbol},
    order::Side,
    pair::TradingPair,
    primitives::{AssetAmount, OrderId, Price, Quantity, UserId},
    trade::Trade,
};
use crate::matching::{
    book::{CancelError, PlacementResult},
    market::{CancelledOrder, Market, MarketCreationError, MarketOrderError, QuoteAmountError},
};

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketOrderRequest {
    Buy {
        quantity: Quantity,
        max_quote_amount: AssetAmount,
    },
    Sell {
        quantity: Quantity,
    },
}

impl MarketOrderRequest {
    fn side(self) -> Side {
        match self {
            Self::Buy { .. } => Side::Buy,
            Self::Sell { .. } => Side::Sell,
        }
    }

    fn quantity(self) -> Quantity {
        match self {
            Self::Buy { quantity, .. } | Self::Sell { quantity } => quantity,
        }
    }
}

pub struct OrderPlacementResult {
    order_id: OrderId,
    outcome: PlacementResult,
}

impl OrderPlacementResult {
    pub fn order_id(&self) -> OrderId {
        self.order_id
    }

    pub fn trades(&self) -> &[Trade] {
        self.outcome.trades()
    }

    pub fn unfilled_quantity(&self) -> Quantity {
        self.outcome.unfilled_quantity()
    }
}

pub struct Exchange {
    assets: HashMap<AssetSymbol, Asset>,
    markets: HashMap<TradingPair, Market>,
    ledger: Ledger,
    next_order_id: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExchangeError {
    AssetAlreadyRegistered,
    UnknownBaseAsset,
    UnknownQuoteAsset,
    MarketAlreadyExists,
    MarketCreation(MarketCreationError),
    UnknownAsset,
    Ledger(LedgerError),
    UnknownMarket,
    OrderIdExhausted,
    MarketOrder(MarketOrderError),
    QuoteAmount(QuoteAmountError),
    SettlementInvariantViolation,
    Cancel(CancelError),
    OrderNotOwnedByUser,
    ZeroMarketBuyBudget,
    MarketBuyBudgetExceeded,
}

impl Exchange {
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
            markets: HashMap::new(),
            ledger: Ledger::new(),
            next_order_id: 1,
        }
    }

    pub fn register_asset(&mut self, asset: Asset) -> Result<(), ExchangeError> {
        let symbol = asset.symbol().clone();
        if self.assets.contains_key(&symbol) {
            return Err(ExchangeError::AssetAlreadyRegistered);
        }
        self.assets.insert(symbol, asset);
        Ok(())
    }

    pub fn create_market(
        &mut self,
        pair: TradingPair,
        price_tick: Price,
        quantity_step: Quantity,
    ) -> Result<(), ExchangeError> {
        if self.markets.contains_key(&pair) {
            return Err(ExchangeError::MarketAlreadyExists);
        }

        let base_asset = self
            .assets
            .get(pair.base())
            .ok_or(ExchangeError::UnknownBaseAsset)?;

        if !self.assets.contains_key(pair.quote()) {
            return Err(ExchangeError::UnknownQuoteAsset);
        }

        let market = Market::new(pair.clone(), base_asset, price_tick, quantity_step)
            .map_err(ExchangeError::MarketCreation)?;

        self.markets.insert(pair, market);

        Ok(())
    }

    pub fn asset(&self, symbol: &AssetSymbol) -> Option<&Asset> {
        self.assets.get(symbol)
    }

    pub fn market(&self, pair: &TradingPair) -> Option<&Market> {
        self.markets.get(pair)
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub fn deposit(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), ExchangeError> {
        if self.asset(asset).is_none() {
            return Err(ExchangeError::UnknownAsset);
        }
        self.ledger
            .deposit(user_id, asset, amount)
            .map_err(ExchangeError::Ledger)
    }

    fn required_lock(
        &self,
        pair: &TradingPair,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> Result<(AssetSymbol, AssetAmount), ExchangeError> {
        let market = self.market(pair).ok_or(ExchangeError::UnknownMarket)?;

        market
            .validate_limit_order(price, quantity)
            .map_err(ExchangeError::MarketOrder)?;

        match side {
            Side::Buy => {
                let amount = market
                    .calculate_quote_amount(price, quantity)
                    .map_err(ExchangeError::QuoteAmount)?;
                Ok(((pair.quote().clone()), amount))
            }
            Side::Sell => {
                let amount = AssetAmount::new(quantity.value());
                Ok((pair.base().clone(), amount))
            }
        }
    }

    fn required_market_lock(
        &self,
        pair: &TradingPair,
        request: MarketOrderRequest,
    ) -> Result<(AssetSymbol, AssetAmount), ExchangeError> {
        let market = self.market(pair).ok_or(ExchangeError::UnknownMarket)?;

        market
            .validate_market_order(request.quantity())
            .map_err(ExchangeError::MarketOrder)?;

        match request {
            MarketOrderRequest::Buy {
                max_quote_amount, ..
            } => {
                if max_quote_amount.value() == 0 {
                    return Err(ExchangeError::ZeroMarketBuyBudget);
                }
                Ok(((pair.quote().clone()), max_quote_amount))
            }
            MarketOrderRequest::Sell { quantity, .. } => {
                Ok((pair.base().clone(), AssetAmount::new(quantity.value())))
            }
        }
    }

    fn settle_generated_trade(
        staged_ledger: &mut Ledger,
        market: &Market,
        pair: &TradingPair,
        trade: &Trade,
    ) -> Result<(), ExchangeError> {
        let (buyer_id, seller_id) = match trade.taker_side() {
            Side::Buy => (trade.taker_user_id(), trade.maker_user_id()),
            Side::Sell => (trade.maker_user_id(), trade.taker_user_id()),
        };

        let base_amount = AssetAmount::new(trade.quantity().value());

        let quote_amount = market
            .calculate_quote_amount(trade.price(), trade.quantity())
            .map_err(ExchangeError::QuoteAmount)?;

        let buyer_quote_refund = match (trade.taker_side(), trade.taker_limit_price()) {
            (Side::Buy, Some(limit_price)) => {
                let reserved_quote = market
                    .calculate_quote_amount(limit_price, trade.quantity())
                    .map_err(ExchangeError::QuoteAmount)?;
                reserved_quote
                    .checked_sub(quote_amount)
                    .ok_or(ExchangeError::SettlementInvariantViolation)?
            }
            _ => AssetAmount::new(0),
        };

        staged_ledger
            .settle_trade(
                buyer_id,
                seller_id,
                pair.base(),
                pair.quote(),
                base_amount,
                quote_amount,
                buyer_quote_refund,
            )
            .map_err(ExchangeError::Ledger)
    }

    pub fn place_limit_order(
        &mut self,
        user_id: UserId,
        pair: &TradingPair,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> Result<OrderPlacementResult, ExchangeError> {
        let (lock_asset, lock_amount) = self.required_lock(pair, side, price, quantity)?;
        let following_order_id = self
            .next_order_id
            .checked_add(1)
            .ok_or(ExchangeError::OrderIdExhausted)?;
        let order_id = OrderId::new(self.next_order_id);

        let mut staged_ledger = self.ledger.clone();
        let mut staged_market = self
            .markets
            .get(pair)
            .ok_or(ExchangeError::UnknownMarket)?
            .clone();
        staged_ledger
            .lock(user_id, &lock_asset, lock_amount)
            .map_err(ExchangeError::Ledger)?;

        let outcome = staged_market
            .place_limit_order(order_id, user_id, side, price, quantity)
            .map_err(ExchangeError::MarketOrder)?;

        for trade in outcome.trades() {
            Self::settle_generated_trade(&mut staged_ledger, &staged_market, pair, trade)?;
        }

        self.ledger = staged_ledger;
        self.markets.insert(pair.clone(), staged_market);
        self.next_order_id = following_order_id;

        Ok(OrderPlacementResult { order_id, outcome })
    }

    pub fn place_market_order(
        &mut self,
        user_id: UserId,
        pair: &TradingPair,
        request: MarketOrderRequest,
    ) -> Result<OrderPlacementResult, ExchangeError> {
        let (lock_asset, lock_amount) = self.required_market_lock(pair, request)?;
        let following_order_id = self
            .next_order_id
            .checked_add(1)
            .ok_or(ExchangeError::OrderIdExhausted)?;
        let order_id = OrderId::new(self.next_order_id);

        let mut staged_ledger = self.ledger.clone();
        let mut staged_market = self
            .markets
            .get(pair)
            .ok_or(ExchangeError::UnknownMarket)?
            .clone();
        staged_ledger
            .lock(user_id, &lock_asset, lock_amount)
            .map_err(ExchangeError::Ledger)?;

        let outcome = staged_market
            .place_market_order(order_id, user_id, request.side(), request.quantity())
            .map_err(ExchangeError::MarketOrder)?;

        let mut total_quote_amount = AssetAmount::new(0);

        for trade in outcome.trades() {
            if trade.taker_order_id() != order_id {
                continue;
            }
            let trade_quote_amount = staged_market
                .calculate_quote_amount(trade.price(), trade.quantity())
                .map_err(ExchangeError::QuoteAmount)?;
            total_quote_amount = total_quote_amount
                .checked_add(trade_quote_amount)
                .ok_or(ExchangeError::QuoteAmount(QuoteAmountError::Overflow))?;
        }

        if let MarketOrderRequest::Buy {
            max_quote_amount, ..
        } = request
        {
            if total_quote_amount.value() > max_quote_amount.value() {
                return Err(ExchangeError::MarketBuyBudgetExceeded);
            }
        }

        for trade in outcome.trades() {
            Self::settle_generated_trade(&mut staged_ledger, &staged_market, pair, trade)?;
        }

        match request {
            MarketOrderRequest::Buy {
                max_quote_amount, ..
            } => {
                let unused_quote = max_quote_amount
                    .checked_sub(total_quote_amount)
                    .ok_or(ExchangeError::SettlementInvariantViolation)?;
                staged_ledger
                    .unlock(user_id, pair.quote(), unused_quote)
                    .map_err(ExchangeError::Ledger)?;
            }
            MarketOrderRequest::Sell { .. } => {
                let unfilled_base = AssetAmount::new(outcome.unfilled_quantity().value());

                staged_ledger
                    .unlock(user_id, pair.base(), unfilled_base)
                    .map_err(ExchangeError::Ledger)?;
            }
        }

        self.ledger = staged_ledger;
        self.markets.insert(pair.clone(), staged_market);
        self.next_order_id = following_order_id;

        Ok(OrderPlacementResult { order_id, outcome })
    }

    pub fn place_stop_limit_order(
        &mut self,
        user_id: UserId,
        pair: &TradingPair,
        side: Side,
        stop_price: Price,
        limit_price: Price,
        quantity: Quantity,
    ) -> Result<OrderId, ExchangeError> {
        let following_order_id = self
            .next_order_id
            .checked_add(1)
            .ok_or(ExchangeError::OrderIdExhausted)?;
        let order_id = OrderId::new(self.next_order_id);

        let mut staged_ledger = self.ledger.clone();
        let mut staged_market = self
            .markets
            .get(pair)
            .ok_or(ExchangeError::UnknownMarket)?
            .clone();

        let (lock_asset, lock_amount) = self.required_lock(pair, side, limit_price, quantity)?;

        staged_market
            .place_stop_limit_order(order_id, user_id, side, stop_price, limit_price, quantity)
            .map_err(ExchangeError::MarketOrder)?;

        staged_ledger
            .lock(user_id, &lock_asset, lock_amount)
            .map_err(ExchangeError::Ledger)?;

        self.ledger = staged_ledger;
        self.markets.insert(pair.clone(), staged_market);
        self.next_order_id = following_order_id;

        Ok(order_id)
    }

    pub fn cancel_order(
        &mut self,
        user_id: UserId,
        pair: &TradingPair,
        order_id: OrderId,
    ) -> Result<CancelledOrder, ExchangeError> {
        let mut staged_ledger = self.ledger.clone();
        let mut staged_market = self
            .markets
            .get(pair)
            .ok_or(ExchangeError::UnknownMarket)?
            .clone();
        let cancelled_order = staged_market
            .cancel(order_id)
            .map_err(ExchangeError::Cancel)?;

        if cancelled_order.user_id() != user_id {
            return Err(ExchangeError::OrderNotOwnedByUser);
        }

        let (unlock_asset, unlock_amount) = match cancelled_order.side() {
            Side::Buy => {
                let price = cancelled_order
                    .limit_price()
                    .ok_or(ExchangeError::SettlementInvariantViolation)?;
                let amount = staged_market
                    .calculate_quote_amount(price, cancelled_order.remaining_quantity())
                    .map_err(ExchangeError::QuoteAmount)?;
                (pair.quote().clone(), amount)
            }
            Side::Sell => {
                let amount = AssetAmount::new(cancelled_order.remaining_quantity().value());
                (pair.base().clone(), amount)
            }
        };

        staged_ledger
            .unlock(user_id, &unlock_asset, unlock_amount)
            .map_err(ExchangeError::Ledger)?;
        self.ledger = staged_ledger;
        self.markets.insert(pair.clone(), staged_market);
        Ok(cancelled_order)
    }
}

#[cfg(test)]
mod tests;
