use crate::asset::{Asset, AssetSymbol};
use crate::book::{CancelError, PlacementResult};
use crate::ledger::{AssetAmount, Ledger, LedgerError};
use crate::market::{Market, MarketCreationError, MarketOrderError, QuoteAmountError};
use crate::order::{Order, Side};
use crate::pair::TradingPair;
use crate::trade::Trade;
use crate::types::{OrderId, Price, Quantity, UserId};
use std::collections::HashMap;

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

    fn settle_generated_trade(
        staged_ledger: &mut Ledger,
        market: &Market,
        pair: &TradingPair,
        incoming_limit_price: Price,
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

        let buyer_quote_refund = match trade.taker_side() {
            Side::Buy => {
                let reserved_quote = market
                    .calculate_quote_amount(incoming_limit_price, trade.quantity())
                    .map_err(ExchangeError::QuoteAmount)?;
                reserved_quote
                    .checked_sub(quote_amount)
                    .ok_or(ExchangeError::SettlementInvariantViolation)?
            }
            Side::Sell => AssetAmount::new(0),
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
            Self::settle_generated_trade(&mut staged_ledger, &staged_market, pair, price, trade)?;
        }

        self.ledger = staged_ledger;
        self.markets.insert(pair.clone(), staged_market);
        self.next_order_id = following_order_id;

        Ok(OrderPlacementResult { order_id, outcome })
    }

    pub fn cancel_order(
        &mut self,
        user_id: UserId,
        pair: &TradingPair,
        order_id: OrderId,
    ) -> Result<Order, ExchangeError> {
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
mod tests {
    use super::*;

    #[test]
    fn registered_asset_can_be_retrieved() {
        let mut exchange = Exchange::new();
        let symbol = AssetSymbol::new("ETH").unwrap();
        let asset = Asset::new(symbol.clone(), 18).unwrap();
        exchange.register_asset(asset).unwrap();

        let stored_asset = exchange.asset(&symbol).unwrap();

        assert_eq!(stored_asset.symbol(), &symbol);
        assert_eq!(stored_asset.decimals(), 18);
    }

    #[test]
    fn duplicate_asset_registration_is_rejected_without_replacement() {
        let mut exchange = Exchange::new();
        let symbol = AssetSymbol::new("ETH").unwrap();
        let original = Asset::new(symbol.clone(), 18).unwrap();
        let conflicting = Asset::new(symbol.clone(), 8).unwrap();
        exchange.register_asset(original).unwrap();

        assert_eq!(
            exchange.register_asset(conflicting),
            Err(ExchangeError::AssetAlreadyRegistered)
        );
        assert_eq!(exchange.asset(&symbol).unwrap().decimals(), 18);
    }

    #[test]
    fn market_can_be_created_from_registered_assets() {
        let mut exchange = Exchange::new();
        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
        let _ = exchange.create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        );

        assert_eq!(exchange.market(&pair).unwrap().pair(), &pair);
    }

    #[test]
    fn market_creation_rejects_unknown_base_asset() {
        let mut exchange = Exchange::new();
        let base_symbol = AssetSymbol::new("ETH").unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
        assert_eq!(
            exchange.create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            ),
            Err(ExchangeError::UnknownBaseAsset)
        )
    }

    #[test]
    fn market_creation_rejects_unknown_quote_asset() {
        let mut exchange = Exchange::new();
        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();

        exchange.register_asset(base_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
        assert_eq!(
            exchange.create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            ),
            Err(ExchangeError::UnknownQuoteAsset)
        )
    }

    #[test]
    fn duplicate_market_creation_is_rejected() {
        let mut exchange = Exchange::new();
        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        assert_eq!(
            exchange.create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            ),
            Err(ExchangeError::MarketAlreadyExists)
        );
    }

    #[test]
    fn deposit_credits_registered_asset_balance() {
        let mut exchange = Exchange::new();
        let alice = UserId::new(1);
        let symbol = AssetSymbol::new("ETH").unwrap();
        let asset = Asset::new(symbol.clone(), 18).unwrap();
        exchange.register_asset(asset).unwrap();

        exchange
            .deposit(alice, &symbol, AssetAmount::new(1_000))
            .unwrap();

        let balance = exchange.ledger().balance(alice, &symbol);

        assert_eq!(balance.available(), AssetAmount::new(1_000));
        assert_eq!(balance.locked(), AssetAmount::new(0));
    }

    #[test]
    fn deposit_rejects_unknown_asset() {
        let mut exchange = Exchange::new();
        let alice = UserId::new(1);
        let symbol = AssetSymbol::new("ETH").unwrap();

        assert_eq!(
            exchange.deposit(alice, &symbol, AssetAmount::new(1_000)),
            Err(ExchangeError::UnknownAsset)
        );
    }

    #[test]
    fn deposit_propagates_ledger_error_without_changing_balance() {
        let mut exchange = Exchange::new();
        let alice = UserId::new(1);
        let usdc = AssetSymbol::new("ETH").unwrap();
        let asset = Asset::new(usdc.clone(), 18).unwrap();
        exchange.register_asset(asset).unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(u128::MAX))
            .unwrap();

        let balance_before = exchange.ledger().balance(alice, &usdc);

        assert_eq!(
            exchange.deposit(alice, &usdc, AssetAmount::new(1)),
            Err(ExchangeError::Ledger(LedgerError::Overflow))
        );

        let balance_after = exchange.ledger().balance(alice, &usdc);

        assert_eq!(balance_after, balance_before);
    }

    #[test]
    fn required_lock_uses_quote_asset_for_buy() {
        let mut exchange = Exchange::new();

        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(3_000_000_000).unwrap(),
                Quantity::new(2_000_000_000_000_000_000),
            )
            .unwrap();

        let required = exchange.required_lock(
            &pair,
            Side::Buy,
            Price::new(3_000_000_000).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        );

        assert_eq!(
            required,
            Ok((pair.quote().clone(), AssetAmount::new(6_000_000_000)))
        );
    }

    #[test]
    fn required_lock_uses_base_asset_for_sell() {
        let mut exchange = Exchange::new();

        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(3_000_000_000).unwrap(),
                Quantity::new(2_000_000_000_000_000_000),
            )
            .unwrap();

        let required = exchange.required_lock(
            &pair,
            Side::Sell,
            Price::new(3_000_000_000).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        );

        assert_eq!(
            required,
            Ok((
                pair.base().clone(),
                AssetAmount::new(2_000_000_000_000_000_000)
            ))
        );
    }

    #[test]
    fn required_lock_rejects_invalid_market_increment() {
        let mut exchange = Exchange::new();

        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(3_000_000_000).unwrap(),
                Quantity::new(2_000_000_000_000_000_000),
            )
            .unwrap();

        assert_eq!(
            exchange.required_lock(
                &pair,
                Side::Sell,
                Price::new(3_000_000_001).unwrap(),
                Quantity::new(2_000_000_000_000_000_000),
            ),
            Err(ExchangeError::MarketOrder(
                MarketOrderError::PriceNotAligned
            ))
        );
    }

    #[test]
    fn required_lock_rejects_unknown_market() {
        let mut exchange = Exchange::new();

        let base_symbol = AssetSymbol::new("ETH").unwrap();
        let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

        let quote_symbol = AssetSymbol::new("USDC").unwrap();
        let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

        exchange.register_asset(base_asset).unwrap();
        exchange.register_asset(quote_asset).unwrap();

        let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

        assert_eq!(
            exchange.required_lock(
                &pair,
                Side::Sell,
                Price::new(3_000_000_001).unwrap(),
                Quantity::new(2_000_000_000_000_000_000),
            ),
            Err(ExchangeError::UnknownMarket)
        );
    }

    #[test]
    fn limit_buy_locks_quote_and_rests_on_market() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth, usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(10_000_000_000))
            .unwrap();
        let price = Price::new(3_000_000_000).unwrap();
        let quantity = Quantity::new(2_000_000_000_000_000_000);

        let result = exchange
            .place_limit_order(alice, &pair, Side::Buy, price, quantity)
            .unwrap();

        assert_eq!(result.order_id(), OrderId::new(1));
        assert!(result.trades().is_empty());
        assert_eq!(result.unfilled_quantity(), quantity);

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        assert_eq!(alice_usdc.locked(), AssetAmount::new(6_000_000_000));
        assert_eq!(alice_usdc.available(), AssetAmount::new(4_000_000_000));

        let order_book = exchange.market(&pair).unwrap().order_book();

        assert_eq!(order_book.best_bid(), Some(price));
        assert_eq!(order_book.best_ask(), None);
    }

    #[test]
    fn crossing_limit_buy_settles_trade_and_refunds_price_improvement() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(4_000_000_000))
            .unwrap();
        exchange
            .deposit(bob, &eth, AssetAmount::new(1_000_000_000_000_000_000))
            .unwrap();

        let bob_price = Price::new(3_000_000_000).unwrap();
        let bob_quantity = Quantity::new(1_000_000_000_000_000_000);

        let _bob_result = exchange
            .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
            .unwrap();

        let alice_price = Price::new(3_100_000_000).unwrap();
        let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

        let result = exchange
            .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
            .unwrap();

        assert_eq!(result.order_id(), OrderId::new(2));
        assert_eq!(result.trades().len(), 1);
        assert_eq!(result.unfilled_quantity(), Quantity::new(0));

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);
        assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
        assert_eq!(alice_usdc.available(), AssetAmount::new(1_000_000_000));
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_eth.available(),
            AssetAmount::new(1_000_000_000_000_000_000)
        );

        let bob_usdc = exchange.ledger().balance(bob, &usdc);
        let bob_eth = exchange.ledger().balance(bob, &eth);
        assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
        assert_eq!(bob_usdc.available(), AssetAmount::new(3_000_000_000));
        assert_eq!(bob_eth.locked(), AssetAmount::new(0));
        assert_eq!(bob_eth.available(), AssetAmount::new(0));

        let order_book = exchange.market(&pair).unwrap().order_book();

        assert_eq!(order_book.best_bid(), None);
        assert_eq!(order_book.best_ask(), None);

        let trade = &result.trades()[0];
        assert_eq!(trade.maker_order_id(), OrderId::new(1));
        assert_eq!(trade.taker_order_id(), OrderId::new(2));
        assert_eq!(trade.maker_user_id(), bob);
        assert_eq!(trade.taker_user_id(), alice);
        assert_eq!(trade.taker_side(), Side::Buy);
        assert_eq!(trade.price(), bob_price);
        assert_eq!(trade.quantity(), alice_quantity);
    }

    #[test]
    fn crossing_limit_sell_settles_at_resting_buy_price() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(3_100_000_000))
            .unwrap();
        exchange
            .deposit(bob, &eth, AssetAmount::new(1_000_000_000_000_000_000))
            .unwrap();

        let alice_price = Price::new(3_100_000_000).unwrap();
        let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

        let _alice_result = exchange
            .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
            .unwrap();

        let bob_price = Price::new(3_000_000_000).unwrap();
        let bob_quantity = Quantity::new(1_000_000_000_000_000_000);

        let result = exchange
            .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
            .unwrap();

        assert_eq!(result.order_id(), OrderId::new(2));
        assert_eq!(result.trades().len(), 1);
        assert_eq!(result.unfilled_quantity(), Quantity::new(0));

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);
        assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
        assert_eq!(alice_usdc.available(), AssetAmount::new(0));
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_eth.available(),
            AssetAmount::new(1_000_000_000_000_000_000)
        );

        let bob_usdc = exchange.ledger().balance(bob, &usdc);
        let bob_eth = exchange.ledger().balance(bob, &eth);
        assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
        assert_eq!(bob_usdc.available(), AssetAmount::new(3_100_000_000));
        assert_eq!(bob_eth.locked(), AssetAmount::new(0));
        assert_eq!(bob_eth.available(), AssetAmount::new(0));

        let order_book = exchange.market(&pair).unwrap().order_book();

        assert_eq!(order_book.best_bid(), None);
        assert_eq!(order_book.best_ask(), None);

        let trade = &result.trades()[0];
        assert_eq!(trade.maker_order_id(), OrderId::new(1));
        assert_eq!(trade.taker_order_id(), OrderId::new(2));
        assert_eq!(trade.maker_user_id(), alice);
        assert_eq!(trade.taker_user_id(), bob);
        assert_eq!(trade.taker_side(), Side::Sell);
        assert_eq!(trade.price(), alice_price);
        assert_eq!(trade.quantity(), alice_quantity);
    }

    #[test]
    fn insufficient_funds_reject_order_without_changing_exchange_state() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        let alice_price = Price::new(3_000_000_000).unwrap();
        let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

        assert!(matches!(
            exchange.place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity),
            Err(ExchangeError::Ledger(LedgerError::InsufficientAvailable))
        ));
        let book = exchange.market(&pair).unwrap().order_book();
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.best_bid(), None);

        let balance = exchange.ledger.balance(alice, &usdc);
        assert_eq!(balance.available(), AssetAmount::new(0));
        assert_eq!(balance.locked(), AssetAmount::new(0));

        exchange
            .deposit(alice, &usdc, AssetAmount::new(3_000_000_000))
            .unwrap();

        let successful = exchange
            .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
            .unwrap();

        assert_eq!(successful.order_id(), OrderId::new(1));
    }

    #[test]
    fn cancelling_partially_filled_buy_unlocks_remaining_quote() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(15_000_000_000))
            .unwrap();
        exchange
            .deposit(bob, &eth, AssetAmount::new(2_000_000_000_000_000_000))
            .unwrap();

        let alice_price = Price::new(3_000_000_000).unwrap();
        let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
        let _alice_result = exchange
            .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
            .unwrap();

        let bob_price = Price::new(3_000_000_000).unwrap();
        let bob_quantity = Quantity::new(2_000_000_000_000_000_000);

        let _bob_result = exchange
            .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
            .unwrap();

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);
        assert_eq!(alice_usdc.locked(), AssetAmount::new(9_000_000_000));
        assert_eq!(alice_usdc.available(), AssetAmount::new(0));
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_eth.available(),
            AssetAmount::new(2_000_000_000_000_000_000)
        );

        let cancelled_order = exchange
            .cancel_order(alice, &pair, OrderId::new(1))
            .unwrap();
        assert_eq!(cancelled_order.id(), OrderId::new(1));
        assert_eq!(
            cancelled_order.original_quantity(),
            Quantity::new(5_000_000_000_000_000_000)
        );
        assert_eq!(
            cancelled_order.remaining_quantity(),
            Quantity::new(3_000_000_000_000_000_000)
        );
        let alice_usdc_after = exchange.ledger().balance(alice, &usdc);
        let alice_eth_after = exchange.ledger().balance(alice, &eth);
        assert_eq!(alice_usdc_after.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_usdc_after.available(),
            AssetAmount::new(9_000_000_000)
        );
        assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_eth_after.available(),
            AssetAmount::new(2_000_000_000_000_000_000)
        );

        let order_book = exchange.market(&pair).unwrap().order_book();

        assert_eq!(order_book.best_bid(), None);
        assert_eq!(order_book.best_ask(), None);
    }

    #[test]
    fn another_user_cannot_cancel_order_or_unlock_its_funds() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &usdc, AssetAmount::new(15_000_000_000))
            .unwrap();

        let alice_price = Price::new(3_000_000_000).unwrap();
        let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
        let _alice_result = exchange
            .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
            .unwrap();

        assert_eq!(
            exchange.cancel_order(bob, &pair, OrderId::new(1)),
            Err(ExchangeError::OrderNotOwnedByUser)
        );

        let alice_usdc = exchange.ledger().balance(alice, &usdc);

        assert_eq!(alice_usdc.available(), AssetAmount::new(0));
        assert_eq!(alice_usdc.locked(), AssetAmount::new(15_000_000_000));

        let order_book = exchange.market(&pair).unwrap().order_book();
        assert_eq!(order_book.best_bid(), Some(alice_price));
        assert_eq!(order_book.best_ask(), None);

        let cancelled = exchange
            .cancel_order(alice, &pair, OrderId::new(1))
            .unwrap();

        assert_eq!(cancelled.id(), OrderId::new(1));
        assert_eq!(cancelled.user_id(), alice);

        let alice_usdc = exchange.ledger().balance(alice, &usdc);

        assert_eq!(alice_usdc.available(), AssetAmount::new(15_000_000_000));
        assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    }

    #[test]
    fn cancelling_sell_unlocks_remaining_base() {
        let mut exchange = Exchange::new();

        let alice = UserId::new(1);
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .deposit(alice, &eth, AssetAmount::new(5_000_000_000_000_000_000))
            .unwrap();

        let alice_price = Price::new(3_000_000_000).unwrap();
        let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
        let _alice_result = exchange
            .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
            .unwrap();

        let cancelled_order = exchange
            .cancel_order(alice, &pair, OrderId::new(1))
            .unwrap();

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);

        assert_eq!(alice_usdc.available(), AssetAmount::new(0));
        assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
        assert_eq!(
            alice_eth.available(),
            AssetAmount::new(5_000_000_000_000_000_000)
        );
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));

        let order_book = exchange.market(&pair).unwrap().order_book();
        assert_eq!(order_book.best_ask(), None);

        assert_eq!(
            cancelled_order.remaining_quantity(),
            Quantity::new(5_000_000_000_000_000_000)
        );
    }

    #[test]
    fn limit_buy_settles_multiple_makers_and_rests_remainder() {
        let mut exchange = Exchange::new();

        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        let alice = UserId::new(1);
        let bob = UserId::new(2);
        let carol = UserId::new(3);
        exchange
            .deposit(alice, &eth, AssetAmount::new(1_000_000_000_000_000_000))
            .unwrap();
        exchange
            .deposit(carol, &eth, AssetAmount::new(2_000_000_000_000_000_000))
            .unwrap();
        exchange
            .deposit(bob, &usdc, AssetAmount::new(12_080_000_000))
            .unwrap();

        let alice_price = Price::new(3_000_000_000).unwrap();
        let alice_quantity = Quantity::new(1_000_000_000_000_000_000);
        let _alice_result = exchange
            .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
            .unwrap();

        let carol_price = Price::new(3_010_000_000).unwrap();
        let carol_quantity = Quantity::new(2_000_000_000_000_000_000);

        let _carol_result = exchange
            .place_limit_order(carol, &pair, Side::Sell, carol_price, carol_quantity)
            .unwrap();

        let bob_usdc = exchange.ledger().balance(bob, &usdc);
        let bob_eth = exchange.ledger().balance(bob, &eth);
        assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
        assert_eq!(bob_usdc.available(), AssetAmount::new(12_080_000_000));
        assert_eq!(bob_eth.locked(), AssetAmount::new(0));
        assert_eq!(bob_eth.available(), AssetAmount::new(0));

        let bob_price = Price::new(3_020_000_000).unwrap();
        let bob_quantity = Quantity::new(4_000_000_000_000_000_000);

        let result = exchange
            .place_limit_order(bob, &pair, Side::Buy, bob_price, bob_quantity)
            .unwrap();

        let trades = result.trades();
        assert_eq!(trades.len(), 2);
        assert_eq!(
            result.unfilled_quantity(),
            Quantity::new(1_000_000_000_000_000_000)
        );

        assert_eq!(trades[0].maker_user_id(), alice);
        assert_eq!(trades[0].taker_user_id(), bob);
        assert_eq!(trades[0].price(), alice_price);
        assert_eq!(trades[0].quantity(), alice_quantity);

        assert_eq!(trades[1].maker_user_id(), carol);
        assert_eq!(trades[1].taker_user_id(), bob);
        assert_eq!(trades[1].price(), carol_price);
        assert_eq!(trades[1].quantity(), carol_quantity);

        let bob_usdc_after = exchange.ledger().balance(bob, &usdc);
        let bob_eth_after = exchange.ledger().balance(bob, &eth);
        assert_eq!(bob_usdc_after.locked(), AssetAmount::new(3_020_000_000));
        assert_eq!(bob_usdc_after.available(), AssetAmount::new(40_000_000));
        assert_eq!(bob_eth_after.locked(), AssetAmount::new(0));
        assert_eq!(
            bob_eth_after.available(),
            AssetAmount::new(3_000_000_000_000_000_000)
        );

        let order_book = exchange.market(&pair).unwrap().order_book();

        assert_eq!(
            order_book.best_bid(),
            Some(Price::new(3_020_000_000).unwrap())
        );
        assert_eq!(order_book.best_ask(), None);

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);

        assert_eq!(alice_usdc.available(), AssetAmount::new(3_000_000_000));
        assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
        assert_eq!(alice_eth.available(), AssetAmount::new(0));
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));

        let carol_usdc = exchange.ledger().balance(carol, &usdc);
        let carol_eth = exchange.ledger().balance(carol, &eth);

        assert_eq!(carol_usdc.available(), AssetAmount::new(6_020_000_000));
        assert_eq!(carol_usdc.locked(), AssetAmount::new(0));
        assert_eq!(carol_eth.available(), AssetAmount::new(0));
        assert_eq!(carol_eth.locked(), AssetAmount::new(0));
    }

    #[test]
    fn markets_keep_order_books_and_locked_funds_isolated() {
        let mut exchange = Exchange::new();

        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();
        let pol = AssetSymbol::new("POL").unwrap();

        exchange
            .register_asset(Asset::new(eth.clone(), 18).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(usdc.clone(), 6).unwrap())
            .unwrap();
        exchange
            .register_asset(Asset::new(pol.clone(), 18).unwrap())
            .unwrap();

        let eth_usdc_pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();
        let pol_usdc_pair = TradingPair::new(pol.clone(), usdc.clone()).unwrap();

        exchange
            .create_market(
                eth_usdc_pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        exchange
            .create_market(
                pol_usdc_pair.clone(),
                Price::new(10_000).unwrap(),
                Quantity::new(100_000_000_000_000),
            )
            .unwrap();

        let alice = UserId::new(1);
        exchange
            .deposit(alice, &usdc, AssetAmount::new(4_000_000_000))
            .unwrap();

        let price_for_eth = Price::new(3_000_000_000).unwrap();
        let price_for_pol = Price::new(500_000).unwrap();
        let eth_quantity = Quantity::new(1_000_000_000_000_000_000);
        let pol_quantity = Quantity::new(100_000_000_000_000_000_000);

        let alice_eth_result = exchange
            .place_limit_order(
                alice,
                &eth_usdc_pair,
                Side::Buy,
                price_for_eth,
                eth_quantity,
            )
            .unwrap();
        assert_eq!(alice_eth_result.order_id(), OrderId::new(1));

        let alice_pol_result = exchange
            .place_limit_order(
                alice,
                &pol_usdc_pair,
                Side::Buy,
                price_for_pol,
                pol_quantity,
            )
            .unwrap();
        assert_eq!(alice_pol_result.order_id(), OrderId::new(2));

        let alice_usdc = exchange.ledger().balance(alice, &usdc);
        let alice_eth = exchange.ledger().balance(alice, &eth);
        let alice_pol = exchange.ledger().balance(alice, &pol);
        assert_eq!(alice_pol.locked(), AssetAmount::new(0));
        assert_eq!(alice_pol.available(), AssetAmount::new(0));
        assert_eq!(alice_eth.locked(), AssetAmount::new(0));
        assert_eq!(alice_eth.available(), AssetAmount::new(0));
        assert_eq!(alice_usdc.locked(), AssetAmount::new(3_050_000_000));
        assert_eq!(alice_usdc.available(), AssetAmount::new(950_000_000));

        assert_eq!(
            exchange
                .market(&eth_usdc_pair)
                .unwrap()
                .order_book()
                .best_bid(),
            Some(price_for_eth)
        );

        assert_eq!(
            exchange
                .market(&pol_usdc_pair)
                .unwrap()
                .order_book()
                .best_bid(),
            Some(price_for_pol)
        );

        let cancelled_order = exchange
            .cancel_order(alice, &eth_usdc_pair, OrderId::new(1))
            .unwrap();
        assert_eq!(cancelled_order.id(), OrderId::new(1));
        assert_eq!(cancelled_order.user_id(), alice);
        assert_eq!(cancelled_order.limit_price(), Some(price_for_eth));

        let alice_usdc_after = exchange.ledger().balance(alice, &usdc);
        let alice_eth_after = exchange.ledger().balance(alice, &eth);
        let alice_pol_after = exchange.ledger().balance(alice, &pol);
        assert_eq!(alice_pol_after.locked(), AssetAmount::new(0));
        assert_eq!(alice_pol_after.available(), AssetAmount::new(0));
        assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));
        assert_eq!(alice_eth_after.available(), AssetAmount::new(0));
        assert_eq!(alice_usdc_after.locked(), AssetAmount::new(50_000_000));
        assert_eq!(
            alice_usdc_after.available(),
            AssetAmount::new(3_950_000_000)
        );

        let eth_usdc_order_book = exchange.market(&eth_usdc_pair).unwrap().order_book();
        let pol_usdc_order_book = exchange.market(&pol_usdc_pair).unwrap().order_book();

        assert_eq!(pol_usdc_order_book.best_bid(), Some(price_for_pol));
        assert_eq!(eth_usdc_order_book.best_bid(), None);
    }
}
