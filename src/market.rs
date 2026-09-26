use crate::asset::Asset;
use crate::book::{CancelError, OrderBook, PlacementResult};
use crate::ledger::AssetAmount;
use crate::order::{Order, OrderError, OrderKind, Side};
use crate::pair::TradingPair;
use crate::types::{OrderId, Price, Quantity, SequenceNumber, UserId};

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
}

#[derive(Clone)]
pub struct Market {
    pair: TradingPair,
    order_book: OrderBook,
    base_scale: u128,
    price_tick: Price,
    quantity_step: Quantity,
    next_sequence_number: u64,
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
            base_scale: base_asset.scale(),
            price_tick,
            quantity_step,
            next_sequence_number: 1,
        })
    }

    pub fn pair(&self) -> &TradingPair {
        &self.pair
    }

    pub fn order_book(&self) -> &OrderBook {
        &self.order_book
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

    fn validate_quantity_and_sequence(&self, quantity: Quantity) -> Result<(), MarketOrderError> {
        if quantity.is_zero() {
            return Err(MarketOrderError::InvalidOrder(OrderError::ZeroQuantity));
        }

        if quantity.value() % self.quantity_step.value() != 0 {
            return Err(MarketOrderError::QuantityNotAligned);
        }

        self.next_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::SequenceExhausted)?;
        Ok(())
    }

    pub(crate) fn validate_limit_order(
        &self,
        price: Price,
        quantity: Quantity,
    ) -> Result<(), MarketOrderError> {
        self.validate_quantity_and_sequence(quantity)?;

        if price.value() % self.price_tick.value() != 0 {
            return Err(MarketOrderError::PriceNotAligned);
        }

        Ok(())
    }

    pub(crate) fn validate_market_order(&self, quantity: Quantity) -> Result<(), MarketOrderError> {
        self.validate_quantity_and_sequence(quantity)
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

        let next_sequence_number = self
            .next_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::SequenceExhausted)?;
        let sequence = SequenceNumber::new(self.next_sequence_number);

        let order = Order::new(
            order_id,
            user_id,
            side,
            OrderKind::Limit { price },
            quantity,
            sequence,
        )
        .map_err(MarketOrderError::InvalidOrder)?;

        self.next_sequence_number = next_sequence_number;
        Ok(self.order_book.place(order))
    }

    pub(crate) fn place_market_order(
        &mut self,
        order_id: OrderId,
        user_id: UserId,
        side: Side,
        quantity: Quantity,
    ) -> Result<PlacementResult, MarketOrderError> {
        self.validate_market_order(quantity)?;

        let next_sequence_number = self
            .next_sequence_number
            .checked_add(1)
            .ok_or(MarketOrderError::SequenceExhausted)?;
        let sequence = SequenceNumber::new(self.next_sequence_number);

        let order = Order::new(
            order_id,
            user_id,
            side,
            OrderKind::Market,
            quantity,
            sequence,
        )
        .map_err(MarketOrderError::InvalidOrder)?;

        self.next_sequence_number = next_sequence_number;
        Ok(self.order_book.place(order))
    }

    pub(crate) fn cancel(&mut self, order_id: OrderId) -> Result<Order, CancelError> {
        self.order_book.cancel(order_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetSymbol;
    use crate::order::Side;
    use crate::types::{SequenceNumber, UserId};

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
        assert_eq!(cancelled.sequence(), SequenceNumber::new(1));

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

        assert_eq!(second.sequence(), SequenceNumber::new(2));
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
        assert_eq!(valid_order.sequence(), SequenceNumber::new(1));
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
}
