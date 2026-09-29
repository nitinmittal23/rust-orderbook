use crate::{
    domain::{
        asset::Asset,
        order::{Order, OrderError, OrderKind, Side},
        pair::TradingPair,
        primitives::{
            AssetAmount, OrderId, Price, Quantity, SequenceNumber, TradeSequenceNumber, UserId,
        },
        stop_order::{StopLimitOrder, StopOrderError},
    },
    matching::{
        book::{CancelError, OrderBook, PlacementResult},
        stop_book::StopOrderBook,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketSnapshot {
    pair: TradingPair,
    last_trade_price: Option<Price>,
    next_order_sequence: SequenceNumber,
    next_trade_sequence: TradeSequenceNumber,
}

impl MarketSnapshot {
    pub fn pair(&self) -> &TradingPair {
        &self.pair
    }

    pub fn last_trade_price(&self) -> Option<Price> {
        self.last_trade_price
    }

    pub fn next_order_sequence(&self) -> SequenceNumber {
        self.next_order_sequence
    }

    pub fn next_trade_sequence(&self) -> TradeSequenceNumber {
        self.next_trade_sequence
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CancelledOrder {
    Active(Order),
    PendingStop(StopLimitOrder),
}

impl CancelledOrder {
    pub fn id(&self) -> OrderId {
        match self {
            Self::Active(order) => order.id(),
            Self::PendingStop(stop) => stop.id(),
        }
    }

    pub fn user_id(&self) -> UserId {
        match self {
            Self::Active(order) => order.user_id(),
            Self::PendingStop(stop) => stop.user_id(),
        }
    }

    pub fn side(&self) -> Side {
        match self {
            Self::Active(order) => order.side(),
            Self::PendingStop(stop) => stop.side(),
        }
    }

    pub fn limit_price(&self) -> Option<Price> {
        match self {
            Self::Active(order) => order.limit_price(),
            Self::PendingStop(stop) => Some(stop.limit_price()),
        }
    }

    pub fn original_quantity(&self) -> Quantity {
        match self {
            Self::Active(order) => order.original_quantity(),
            Self::PendingStop(stop) => stop.quantity(),
        }
    }

    pub fn remaining_quantity(&self) -> Quantity {
        match self {
            Self::Active(order) => order.remaining_quantity(),
            Self::PendingStop(stop) => stop.quantity(),
        }
    }

    pub fn stop_price(&self) -> Option<Price> {
        match self {
            Self::Active(_) => None,
            Self::PendingStop(stop) => Some(stop.stop_price()),
        }
    }

    pub fn sequence(&self) -> Option<SequenceNumber> {
        match self {
            Self::Active(order) => Some(order.sequence()),
            Self::PendingStop(_) => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum QuoteAmountError {
    Overflow,
    InexactAmount,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MarketCreationError {
    BaseAssetMismatch,
    ZeroQuantityStep,
    IncrementCalculationOverflow,
    IncompatibleIncrements,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MarketOrderError {
    SequenceExhausted,
    InvalidOrder(OrderError),
    PriceNotAligned,
    QuantityNotAligned,
    InvalidStopOrder(StopOrderError),
    StopWouldTriggerImmediately,
    TradeSequenceExhausted,
}

#[derive(Clone)]
pub struct Market {
    pair: TradingPair,
    order_book: OrderBook,
    stop_order_book: StopOrderBook,
    last_trade_price: Option<Price>,
    base_scale: u128,
    price_tick: Price,
    quantity_step: Quantity,
    next_sequence_number: u64,
    next_trade_sequence_number: u64,
}

impl Market {
    pub fn new(
        pair: TradingPair,
        base_asset: &Asset,
        price_tick: Price,
        quantity_step: Quantity,
    ) -> Result<Self, MarketCreationError> {
        if pair.base() != base_asset.symbol() {
            return Err(MarketCreationError::BaseAssetMismatch);
        }

        if quantity_step.is_zero() {
            return Err(MarketCreationError::ZeroQuantityStep);
        }

        let smallest_trade_product = price_tick
            .value()
            .checked_mul(quantity_step.value())
            .ok_or(MarketCreationError::IncrementCalculationOverflow)?;

        if smallest_trade_product % base_asset.scale() != 0 {
            return Err(MarketCreationError::IncompatibleIncrements);
        }

        Ok(Self {
            pair,
            order_book: OrderBook::new(),
            stop_order_book: StopOrderBook::default(),
            last_trade_price: None,
            base_scale: base_asset.scale(),
            price_tick,
            quantity_step,
            next_sequence_number: 1,
            next_trade_sequence_number: 1,
        })
    }

    pub fn pair(&self) -> &TradingPair {
        &self.pair
    }

    pub fn order_book(&self) -> &OrderBook {
        &self.order_book
    }

    pub fn last_trade_price(&self) -> Option<Price> {
        self.last_trade_price
    }

    pub(crate) fn snapshot(&self) -> MarketSnapshot {
        MarketSnapshot {
            pair: self.pair.clone(),
            last_trade_price: self.last_trade_price,
            next_order_sequence: SequenceNumber::new(self.next_sequence_number),
            next_trade_sequence: TradeSequenceNumber::new(self.next_trade_sequence_number),
        }
    }

    fn allocate_trade_sequence(&mut self) -> Result<TradeSequenceNumber, MarketOrderError> {
        let following_sequence = self
            .next_trade_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::TradeSequenceExhausted)?;

        let sequence = TradeSequenceNumber::new(self.next_trade_sequence_number);
        self.next_trade_sequence_number = following_sequence;

        Ok(sequence)
    }

    pub fn calculate_quote_amount(
        &self,
        price: Price,
        quantity: Quantity,
    ) -> Result<AssetAmount, QuoteAmountError> {
        let product = price
            .value()
            .checked_mul(quantity.value())
            .ok_or(QuoteAmountError::Overflow)?;
        let scale = self.base_scale;
        if product % scale != 0 {
            return Err(QuoteAmountError::InexactAmount);
        }
        Ok(AssetAmount::new(product / scale))
    }

    fn validate_quantity(&self, quantity: Quantity) -> Result<(), MarketOrderError> {
        if quantity.is_zero() {
            return Err(MarketOrderError::InvalidOrder(OrderError::ZeroQuantity));
        }

        if quantity.value() % self.quantity_step.value() != 0 {
            return Err(MarketOrderError::QuantityNotAligned);
        }
        Ok(())
    }

    fn validate_sequence_available(&self) -> Result<(), MarketOrderError> {
        self.next_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::SequenceExhausted)?;
        Ok(())
    }

    pub(crate) fn validate_price(&self, price: Price) -> Result<(), MarketOrderError> {
        if price.value() % self.price_tick.value() != 0 {
            return Err(MarketOrderError::PriceNotAligned);
        }

        Ok(())
    }

    pub(crate) fn validate_limit_order(
        &self,
        price: Price,
        quantity: Quantity,
    ) -> Result<(), MarketOrderError> {
        self.validate_quantity(quantity)?;
        self.validate_sequence_available()?;
        self.validate_price(price)?;
        Ok(())
    }

    pub(crate) fn validate_market_order(&self, quantity: Quantity) -> Result<(), MarketOrderError> {
        self.validate_quantity(quantity)?;
        self.validate_sequence_available()?;
        Ok(())
    }

    fn allocate_sequence(&mut self) -> Result<SequenceNumber, MarketOrderError> {
        let next_sequence_number = self
            .next_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::SequenceExhausted)?;
        let sequence = SequenceNumber::new(self.next_sequence_number);
        self.next_sequence_number = next_sequence_number;
        Ok(sequence)
    }

    fn place_active_order(&mut self, order: Order) -> Result<PlacementResult, MarketOrderError> {
        let mut outcome = self.order_book.place(order);
        let mut next_trade_index = 0;

        while next_trade_index < outcome.trades().len() {
            let trade_sequence = self.allocate_trade_sequence()?;

            let trade = &mut outcome.trades_mut()[next_trade_index];
            trade.assign_sequence(trade_sequence);

            let trade_price = trade.price();
            next_trade_index += 1;

            self.last_trade_price = Some(trade_price);
            let triggered_stops = self.stop_order_book.take_triggered(trade_price);

            for stop in triggered_stops {
                let sequence = self.allocate_sequence()?;
                let active_order = stop
                    .into_active_order(sequence)
                    .map_err(MarketOrderError::InvalidOrder)?;
                let triggered_outcome = self.order_book.place(active_order);
                outcome.append_changes_from(triggered_outcome);
            }
        }

        Ok(outcome)
    }

    pub(crate) fn place_limit_order(
        &mut self,
        order_id: OrderId,
        user_id: UserId,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> Result<PlacementResult, MarketOrderError> {
        self.validate_limit_order(price, quantity)?;

        let sequence = self.allocate_sequence()?;

        let order = Order::new(
            order_id,
            user_id,
            side,
            OrderKind::Limit { price },
            quantity,
            sequence,
        )
        .map_err(MarketOrderError::InvalidOrder)?;

        self.place_active_order(order)
    }

    pub(crate) fn place_market_order(
        &mut self,
        order_id: OrderId,
        user_id: UserId,
        side: Side,
        quantity: Quantity,
    ) -> Result<PlacementResult, MarketOrderError> {
        self.validate_market_order(quantity)?;

        let sequence = self.allocate_sequence()?;

        let order = Order::new(
            order_id,
            user_id,
            side,
            OrderKind::Market,
            quantity,
            sequence,
        )
        .map_err(MarketOrderError::InvalidOrder)?;

        self.place_active_order(order)
    }

    pub(crate) fn place_stop_limit_order(
        &mut self,
        order_id: OrderId,
        user_id: UserId,
        side: Side,
        stop_price: Price,
        limit_price: Price,
        quantity: Quantity,
    ) -> Result<(), MarketOrderError> {
        let order = StopLimitOrder::new(order_id, user_id, side, stop_price, limit_price, quantity)
            .map_err(MarketOrderError::InvalidStopOrder)?;

        self.validate_quantity(quantity)?;
        self.validate_price(stop_price)?;
        self.validate_price(limit_price)?;

        if self
            .last_trade_price
            .is_some_and(|price| order.is_triggered_by(price))
        {
            return Err(MarketOrderError::StopWouldTriggerImmediately);
        }

        self.stop_order_book.add(order);
        Ok(())
    }

    pub(crate) fn cancel(&mut self, order_id: OrderId) -> Result<CancelledOrder, CancelError> {
        if let Ok(order) = self.order_book.cancel(order_id) {
            return Ok(CancelledOrder::Active(order));
        }
        self.stop_order_book
            .cancel(order_id)
            .map(CancelledOrder::PendingStop)
            .ok_or(CancelError::OrderNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::asset::AssetSymbol;
    use crate::domain::order::Side;
    use crate::domain::primitives::{SequenceNumber, UserId};

    #[test]
    fn quote_amount_is_calculated_in_quote_atomic_units() {
        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 18).unwrap();
        let price = Price::new(3_000_250_000).unwrap();
        let quantity = Quantity::new(1_500_000_000_000_000_000);
        let price_tick = Price::new(10_000).unwrap();
        let quantity_step = Quantity::new(100_000_000_000_000);

        let market = Market::new(
            TradingPair::new(
                AssetSymbol::new("ETH").unwrap(),
                AssetSymbol::new("USDC").unwrap(),
            )
            .unwrap(),
            &eth,
            price_tick,
            quantity_step,
        )
        .unwrap();

        assert_eq!(
            market.calculate_quote_amount(price, quantity),
            Ok(AssetAmount::new(4_500_375_000))
        );
    }

    #[test]
    fn quote_amount_overflow_is_rejected() {
        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let pair = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let price_tick = Price::new(10_000).unwrap();
        let quantity_step = Quantity::new(100_000_000_000_000);

        let market = Market::new(pair, &eth, price_tick, quantity_step).unwrap();

        let price = Price::new(u128::MAX).unwrap();
        let quantity = Quantity::new(2);

        assert_eq!(
            market.calculate_quote_amount(price, quantity),
            Err(QuoteAmountError::Overflow)
        );
    }

    #[test]
    fn market_rejects_mismatched_base_asset() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let btc = Asset::new(AssetSymbol::new("BTC").unwrap(), 8).unwrap();
        assert!(matches!(
            Market::new(
                eth_usdc,
                &btc,
                Price::new(1).unwrap(),
                Quantity::new(100_000_000),
            ),
            Err(MarketCreationError::BaseAssetMismatch)
        ));
    }

    #[test]
    fn inexact_quote_amount_is_rejected() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price = Price::new(1).unwrap();
        let quantity = Quantity::new(50);
        let price_tick = Price::new(4).unwrap();
        let quantity_step = Quantity::new(25);

        let market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        assert_eq!(
            market.calculate_quote_amount(price, quantity),
            Err(QuoteAmountError::InexactAmount)
        );
    }

    #[test]
    fn market_places_and_cancels_order_through_its_order_book() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(20);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        let result = market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();
        assert!(result.trades().is_empty());
        assert_eq!(
            market.order_book().best_bid(),
            Some(Price::new(100).unwrap())
        );

        let cancelled = market.cancel(OrderId::new(1)).unwrap();
        assert_eq!(cancelled.id(), OrderId::new(1));
        assert_eq!(market.order_book().best_bid(), None);
        assert_eq!(cancelled.sequence(), Some(SequenceNumber::new(1)));

        market
            .place_limit_order(
                OrderId::new(2),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        let second = market.cancel(OrderId::new(2)).unwrap();

        assert_eq!(second.sequence(), Some(SequenceNumber::new(2)));
    }

    #[test]
    fn invalid_market_increments_do_not_consume_sequence_numbers() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(20);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        assert_eq!(
            market.place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Buy,
                Price::new(101).unwrap(),
                Quantity::new(40),
            ),
            Err(MarketOrderError::PriceNotAligned)
        );

        assert_eq!(
            market.place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(30),
            ),
            Err(MarketOrderError::QuantityNotAligned)
        );

        assert_eq!(market.order_book().best_bid(), None);
        assert_eq!(market.order_book().best_ask(), None);

        market
            .place_limit_order(
                OrderId::new(3),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(40),
            )
            .unwrap();

        let valid_order = market.cancel(OrderId::new(3)).unwrap();
        assert_eq!(valid_order.sequence(), Some(SequenceNumber::new(1)));
    }

    #[test]
    fn zero_quantity_step_is_rejected() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(0);

        assert!(matches!(
            Market::new(eth_usdc, &eth, price_tick, quantity_step),
            Err(MarketCreationError::ZeroQuantityStep)
        ));
    }

    #[test]
    fn incompatible_market_increments_are_rejected() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price_tick = Price::new(1).unwrap();
        let quantity_step = Quantity::new(50);

        assert!(matches!(
            Market::new(eth_usdc, &eth, price_tick, quantity_step),
            Err(MarketCreationError::IncompatibleIncrements)
        ));
    }

    #[test]
    fn market_increment_multiplication_overflow_is_rejected() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 2).unwrap();
        let price_tick = Price::new(u128::MAX).unwrap();
        let quantity_step = Quantity::new(50);

        assert!(matches!(
            Market::new(eth_usdc, &eth, price_tick, quantity_step),
            Err(MarketCreationError::IncrementCalculationOverflow)
        ));
    }

    #[test]
    fn market_order_matches_and_does_not_rest_remainder() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 18).unwrap();

        let price_tick = Price::new(10_000).unwrap();
        let quantity_step = Quantity::new(100_000_000_000_000);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();
        let resting_price = Price::new(3_000_000_000).unwrap();

        let alice_result = market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Sell,
                resting_price,
                Quantity::new(1_000_000_000_000_000_000),
            )
            .unwrap();
        assert!(alice_result.trades().is_empty());
        assert_eq!(market.order_book().best_ask(), Some(resting_price));

        let result = market
            .place_market_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Buy,
                Quantity::new(2_000_000_000_000_000_000),
            )
            .unwrap();
        assert_eq!(result.trades().len(), 1);

        let trade = &result.trades()[0];
        assert_eq!(trade.maker_user_id(), UserId::new(1));
        assert_eq!(trade.taker_user_id(), UserId::new(2));
        assert_eq!(trade.taker_side(), Side::Buy);
        assert_eq!(trade.price(), resting_price);
        assert_eq!(trade.quantity(), Quantity::new(1_000_000_000_000_000_000));
        assert_eq!(
            result.unfilled_quantity(),
            Quantity::new(1_000_000_000_000_000_000)
        );

        let order_book = market.order_book();
        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);
    }

    #[test]
    fn stop_limit_order_remains_pending_and_does_not_consume_sequence() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(20);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        market
            .place_stop_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Sell,
                Price::new(100).unwrap(),
                Price::new(95).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        assert_eq!(market.order_book().best_ask(), None);
        assert_eq!(market.order_book().best_bid(), None);

        let orders = market
            .stop_order_book
            .take_triggered(Price::new(100).unwrap());
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].id(), OrderId::new(1));

        market
            .place_limit_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Sell,
                Price::new(100).unwrap(),
                Quantity::new(100),
            )
            .unwrap();
        let cancelled_order = market.cancel(OrderId::new(2)).unwrap();
        assert_eq!(cancelled_order.id(), OrderId::new(2));
        assert_eq!(cancelled_order.sequence(), Some(SequenceNumber::new(1)));
    }

    #[test]
    fn trade_triggers_stop_limit_and_processes_its_generated_trade() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(20);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        let bob_result = market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(40),
            )
            .unwrap();
        assert!(bob_result.trades().is_empty());
        assert_eq!(
            market.order_book().best_bid(),
            Some(Price::new(100).unwrap())
        );

        market
            .place_stop_limit_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Sell,
                Price::new(100).unwrap(),
                Price::new(95).unwrap(),
                Quantity::new(40),
            )
            .unwrap();

        let carol_result = market
            .place_limit_order(
                OrderId::new(3),
                UserId::new(3),
                Side::Sell,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();
        assert_eq!(carol_result.trades().len(), 2);
        assert_eq!(carol_result.unfilled_quantity(), Quantity::new(0));

        let trades = carol_result.trades();

        assert_eq!(trades[0].maker_order_id(), OrderId::new(1));
        assert_eq!(trades[0].maker_user_id(), UserId::new(1));
        assert_eq!(trades[0].taker_order_id(), OrderId::new(3));
        assert_eq!(trades[0].taker_user_id(), UserId::new(3));
        assert_eq!(trades[0].taker_side(), Side::Sell);
        assert_eq!(trades[0].price(), Price::new(100).unwrap());
        assert_eq!(trades[0].quantity(), Quantity::new(20));

        assert_eq!(trades[1].maker_order_id(), OrderId::new(1));
        assert_eq!(trades[1].maker_user_id(), UserId::new(1));
        assert_eq!(trades[1].taker_order_id(), OrderId::new(2));
        assert_eq!(trades[1].taker_user_id(), UserId::new(2));
        assert_eq!(trades[1].taker_side(), Side::Sell);
        assert_eq!(trades[1].price(), Price::new(100).unwrap());
        assert_eq!(trades[1].quantity(), Quantity::new(20));

        assert_eq!(market.last_trade_price(), Some(Price::new(100).unwrap()));
        assert_eq!(market.order_book().best_bid(), None);

        assert_eq!(
            market.order_book().best_ask(),
            Some(Price::new(95).unwrap())
        );
        let alice_remainder = market.cancel(OrderId::new(2)).unwrap();

        assert_eq!(alice_remainder.sequence(), Some(SequenceNumber::new(3)));
        assert_eq!(alice_remainder.remaining_quantity(), Quantity::new(20));
        assert_eq!(alice_remainder.limit_price(), Some(Price::new(95).unwrap()));
    }

    #[test]
    fn stop_limit_that_would_trigger_immediately_is_rejected() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let price_tick = Price::new(5).unwrap();
        let quantity_step = Quantity::new(20);
        let mut market = Market::new(eth_usdc, &eth, price_tick, quantity_step).unwrap();

        let bob_result = market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(40),
            )
            .unwrap();
        assert!(bob_result.trades().is_empty());
        assert_eq!(
            market.order_book().best_bid(),
            Some(Price::new(100).unwrap())
        );

        let carol_result = market
            .place_limit_order(
                OrderId::new(3),
                UserId::new(3),
                Side::Sell,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();
        assert_eq!(carol_result.trades().len(), 1);
        assert_eq!(carol_result.unfilled_quantity(), Quantity::new(0));

        let trades = carol_result.trades();

        assert_eq!(trades[0].maker_order_id(), OrderId::new(1));
        assert_eq!(trades[0].maker_user_id(), UserId::new(1));
        assert_eq!(trades[0].taker_order_id(), OrderId::new(3));
        assert_eq!(trades[0].taker_user_id(), UserId::new(3));
        assert_eq!(trades[0].taker_side(), Side::Sell);
        assert_eq!(trades[0].price(), Price::new(100).unwrap());
        assert_eq!(trades[0].quantity(), Quantity::new(20));

        assert_eq!(
            market.place_stop_limit_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Sell,
                Price::new(110).unwrap(),
                Price::new(105).unwrap(),
                Quantity::new(40),
            ),
            Err(MarketOrderError::StopWouldTriggerImmediately)
        );

        assert_eq!(market.stop_order_book.cancel(OrderId::new(2)), None);

        assert_eq!(market.order_book().best_ask(), None);
        assert_eq!(
            market.order_book().best_bid(),
            Some(Price::new(100).unwrap())
        );
        assert_eq!(market.last_trade_price(), Some(Price::new(100).unwrap()));

        market
            .place_limit_order(
                OrderId::new(4),
                UserId::new(4),
                Side::Sell,
                Price::new(105).unwrap(),
                Quantity::new(20),
            )
            .unwrap();
        let next_order = market.cancel(OrderId::new(4)).unwrap();
        assert_eq!(next_order.sequence(), Some(SequenceNumber::new(3)));
    }

    #[test]
    fn buy_stop_limit_remains_pending_below_trigger_price() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let mut market =
            Market::new(eth_usdc, &eth, Price::new(5).unwrap(), Quantity::new(20)).unwrap();

        market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Sell,
                Price::new(90).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        market
            .place_limit_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Buy,
                Price::new(90).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        assert_eq!(market.last_trade_price(), Some(Price::new(90).unwrap()));

        market
            .place_stop_limit_order(
                OrderId::new(3),
                UserId::new(3),
                Side::Buy,
                Price::new(100).unwrap(),
                Price::new(105).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        assert_eq!(market.order_book().best_ask(), None);
        assert_eq!(market.order_book().best_bid(), None);

        let cancelled = market.cancel(OrderId::new(3)).unwrap();

        assert!(matches!(&cancelled, CancelledOrder::PendingStop(_)));
        assert_eq!(cancelled.side(), Side::Buy);
        assert_eq!(cancelled.stop_price(), Some(Price::new(100).unwrap()));
        assert_eq!(cancelled.limit_price(), Some(Price::new(105).unwrap()));
        assert_eq!(cancelled.sequence(), None);
    }

    #[test]
    fn trade_at_or_above_stop_price_activates_buy_stop() {
        let eth_usdc = TradingPair::new(
            AssetSymbol::new("ETH").unwrap(),
            AssetSymbol::new("USDC").unwrap(),
        )
        .unwrap();

        let eth = Asset::new(AssetSymbol::new("ETH").unwrap(), 0).unwrap();

        let mut market =
            Market::new(eth_usdc, &eth, Price::new(5).unwrap(), Quantity::new(20)).unwrap();

        market
            .place_limit_order(
                OrderId::new(1),
                UserId::new(1),
                Side::Sell,
                Price::new(90).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        market
            .place_limit_order(
                OrderId::new(2),
                UserId::new(2),
                Side::Buy,
                Price::new(90).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        market
            .place_stop_limit_order(
                OrderId::new(3),
                UserId::new(3),
                Side::Buy,
                Price::new(100).unwrap(),
                Price::new(105).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        market
            .place_limit_order(
                OrderId::new(4),
                UserId::new(4),
                Side::Sell,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        let outcome = market
            .place_limit_order(
                OrderId::new(5),
                UserId::new(5),
                Side::Buy,
                Price::new(100).unwrap(),
                Quantity::new(20),
            )
            .unwrap();

        assert_eq!(outcome.trades().len(), 1);
        assert_eq!(outcome.trades()[0].price(), Price::new(100).unwrap());
        assert_eq!(market.last_trade_price(), Some(Price::new(100).unwrap()));

        assert_eq!(
            market.order_book().best_bid(),
            Some(Price::new(105).unwrap())
        );
        assert_eq!(market.order_book().best_ask(), None);

        let cancelled = market.cancel(OrderId::new(3)).unwrap();

        assert!(matches!(&cancelled, CancelledOrder::Active(_)));
        assert_eq!(cancelled.side(), Side::Buy);
        assert_eq!(cancelled.limit_price(), Some(Price::new(105).unwrap()));

        assert_eq!(cancelled.sequence(), Some(SequenceNumber::new(5)));
    }
}
