use crate::order::{Order, OrderKind, Side};
use crate::trade::Trade;
use crate::types::{OrderId, Price, Quantity};
use std::cmp::min;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, PartialEq, Eq)]
struct PriceLevel {
    price: Price,
    orders: VecDeque<Order>,
}

#[derive(Debug, PartialEq, Eq)]
enum PriceLevelError {
    PriceMismatch,
    MarketOrderCannotRest,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CancelError {
    OrderNotFound,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OrderBook {
    bids: BTreeMap<Price, PriceLevel>,
    asks: BTreeMap<Price, PriceLevel>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlacementResult {
    trades: Vec<Trade>,
    unfilled_quantity: Quantity,
}

impl PlacementResult {
    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }

    pub fn unfilled_quantity(&self) -> Quantity {
        self.unfilled_quantity
    }
}

impl PriceLevel {
    fn new(price: Price) -> Self {
        PriceLevel {
            price,
            orders: VecDeque::new(),
        }
    }

    fn price(&self) -> Price {
        self.price
    }

    fn len(&self) -> usize {
        self.orders.len()
    }

    fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    fn add(&mut self, order: Order) -> Result<(), PriceLevelError> {
        let price = match order.limit_price() {
            Some(price) => price,
            None => return Err(PriceLevelError::MarketOrderCannotRest),
        };

        if price != self.price {
            return Err(PriceLevelError::PriceMismatch);
        }
        self.orders.push_back(order);
        Ok(())
    }

    fn front(&self) -> Option<&Order> {
        self.orders.front()
    }

    fn front_mut(&mut self) -> Option<&mut Order> {
        self.orders.front_mut()
    }

    fn pop_front(&mut self) -> Option<Order> {
        self.orders.pop_front()
    }

    fn remove(&mut self, order_id: OrderId) -> Option<Order> {
        let index = self
            .orders
            .iter()
            .position(|order| order.id() == order_id)?;
        self.orders.remove(index)
    }
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    fn add_resting_order(&mut self, order: Order) -> Result<(), PriceLevelError> {
        let price = match order.limit_price() {
            Some(price) => price,
            None => return Err(PriceLevelError::MarketOrderCannotRest),
        };

        let levels = match order.side() {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        let price_level = levels
            .entry(price)
            .or_insert_with(|| PriceLevel::new(price));
        price_level.add(order)
    }

    pub fn best_bid(&self) -> Option<Price> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.asks.keys().next().copied()
    }

    fn crosses(incoming: &Order, resting_price: Price) -> bool {
        match incoming.limit_price() {
            None => true,
            Some(incoming_price) => match incoming.side() {
                Side::Buy => incoming_price >= resting_price,
                Side::Sell => incoming_price <= resting_price,
            },
        }
    }

    fn execute_trade(incoming: &mut Order, resting: &mut Order) -> Trade {
        assert!(!incoming.is_filled(), "incoming order is already filled");
        assert!(!resting.is_filled(), "resting order is already filled");
        assert_ne!(incoming.side(), resting.side());
        let resting_price = resting
            .limit_price()
            .expect("resting order must be a limit order");
        assert!(Self::crosses(incoming, resting_price));

        let quantity = min(incoming.remaining_quantity(), resting.remaining_quantity());

        let trade = Trade::new(
            resting.id(),
            incoming.id(),
            resting.user_id(),
            incoming.user_id(),
            incoming.side(),
            resting_price,
            quantity,
        );

        incoming
            .fill(quantity)
            .expect("execution quantity cannot exceed incoming quantity");
        resting
            .fill(quantity)
            .expect("execution quantity cannot exceed resting quantity");

        trade
    }

    pub fn place(&mut self, mut incoming: Order) -> PlacementResult {
        let mut trades: Vec<Trade> = Vec::new();
        while !incoming.is_filled() {
            let best_price = match incoming.side() {
                Side::Buy => self.best_ask(),
                Side::Sell => self.best_bid(),
            };
            let Some(best_price) = best_price else {
                break;
            };
            if !Self::crosses(&incoming, best_price) {
                break;
            }

            let opposite_levels = match incoming.side() {
                Side::Buy => &mut self.asks,
                Side::Sell => &mut self.bids,
            };

            let (trade, level_is_empty) = {
                let level = opposite_levels
                    .get_mut(&best_price)
                    .expect("best price must have a price level");
                let (trade, maker_is_filled) = {
                    let resting_order = level
                        .front_mut()
                        .expect("price level must contain an order");
                    let trade = Self::execute_trade(&mut incoming, resting_order);
                    (trade, resting_order.is_filled())
                };

                if maker_is_filled {
                    level.pop_front().expect("filled maker must exit at front");
                }

                (trade, level.is_empty())
            };

            if level_is_empty {
                opposite_levels.remove(&best_price);
            }

            trades.push(trade);
        }

        let unfilled_quantity = incoming.remaining_quantity();
        if !incoming.is_filled() {
            match incoming.kind() {
                OrderKind::Limit { .. } => {
                    self.add_resting_order(incoming)
                        .expect("incoming remainder must match its own price level");
                }
                OrderKind::Market => {}
            }
        }

        PlacementResult {
            trades,
            unfilled_quantity,
        }
    }

    fn remove_from_levels(
        levels: &mut BTreeMap<Price, PriceLevel>,
        order_id: OrderId,
    ) -> Option<Order> {
        let mut removed_order = None;
        let mut empty_price = None;
        for (price, level) in levels.iter_mut() {
            if let Some(order) = level.remove(order_id) {
                if level.is_empty() {
                    empty_price = Some(*price);
                }
                removed_order = Some(order);
                break;
            }
        }

        if let Some(price) = empty_price {
            levels.remove(&price);
        }

        removed_order
    }

    pub fn cancel(&mut self, order_id: OrderId) -> Result<Order, CancelError> {
        if let Some(order) = Self::remove_from_levels(&mut self.bids, order_id) {
            return Ok(order);
        }

        if let Some(order) = Self::remove_from_levels(&mut self.asks, order_id) {
            return Ok(order);
        }

        Err(CancelError::OrderNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderKind;
    use crate::types::{OrderId, Quantity, SequenceNumber, UserId};

    #[test]
    fn new_price_level_has_its_price_and_no_orders() {
        let price = Price::new(100).unwrap();
        let level = PriceLevel::new(price);
        assert_eq!(level.price(), price);
        assert_eq!(level.len(), 0);
        assert!(level.is_empty());
    }

    fn make_order(
        id: u64,
        user_id: UserId,
        price: Price,
        sequence: u64,
        side: Side,
        quantity: Quantity,
    ) -> Order {
        Order::new(
            OrderId::new(id),
            user_id,
            side,
            OrderKind::Limit { price },
            quantity,
            SequenceNumber::new(sequence),
        )
        .unwrap()
    }

    #[test]
    fn same_price_order_is_added() {
        let price = Price::new(100).unwrap();
        let mut level = PriceLevel::new(price);
        let order = make_order(1, UserId::new(1), price, 1, Side::Buy, Quantity::new(10));
        let order_id = order.id();

        assert_eq!(level.add(order), Ok(()));
        assert_eq!(level.len(), 1);
        assert_eq!(level.front().unwrap().id(), order_id);
    }

    #[test]
    fn different_price_order_is_rejected() {
        let price = Price::new(100).unwrap();
        let mut level = PriceLevel::new(price);
        let order = make_order(
            1,
            UserId::new(1),
            Price::new(200).unwrap(),
            1,
            Side::Buy,
            Quantity::new(10),
        );
        assert_eq!(level.add(order), Err(PriceLevelError::PriceMismatch));
        assert!(level.is_empty());
        assert_eq!(level.len(), 0);
        assert!(level.front().is_none());
    }

    #[test]
    fn orders_at_same_price_are_fifo() {
        let price = Price::new(100).unwrap();
        let mut level = PriceLevel::new(price);
        let order1 = make_order(1, UserId::new(1), price, 1, Side::Buy, Quantity::new(10));
        let order2 = make_order(2, UserId::new(1), price, 2, Side::Buy, Quantity::new(10));

        let order1_id = order1.id();
        let order2_id = order2.id();

        level.add(order1).unwrap();
        level.add(order2).unwrap();

        assert_eq!(level.front().unwrap().id(), order1_id);
        let popped_order = level.pop_front().unwrap();
        assert_eq!(popped_order.id(), order1_id);
        assert_eq!(level.front().unwrap().id(), order2_id);
        let second_popped_order = level.pop_front().unwrap();
        assert_eq!(second_popped_order.id(), order2_id);
        assert!(level.is_empty());
    }

    #[test]
    fn front_order_can_be_partially_filled() {
        let price = Price::new(100).unwrap();
        let mut level = PriceLevel::new(price);
        level
            .add(make_order(
                1,
                UserId::new(1),
                price,
                1,
                Side::Buy,
                Quantity::new(10),
            ))
            .unwrap();
        let front_order = level.front_mut().unwrap();
        front_order.fill(Quantity::new(6)).unwrap();

        assert_eq!(
            level.front().unwrap().remaining_quantity(),
            Quantity::new(4)
        );
        assert_eq!(level.len(), 1);
    }

    #[test]
    fn new_book_contains_no_bids_or_asks() {
        let book = OrderBook::new();
        assert!(book.bids.is_empty());
        assert!(book.asks.is_empty());
    }

    #[test]
    fn buy_order_rests_on_bid_side() {
        let mut book = OrderBook::new();
        let price = Price::new(100).unwrap();
        let order = make_order(1, UserId::new(1), price, 1, Side::Buy, Quantity::new(10));
        let order_id = order.id();
        book.add_resting_order(order).unwrap();

        let level = book.bids.get(&price).unwrap();
        assert_eq!(book.bids.len(), 1);
        assert_eq!(level.price(), price);
        assert_eq!(level.front().unwrap().id(), order_id);
        assert_eq!(book.asks.len(), 0);
    }

    #[test]
    fn sell_order_rests_on_ask_side() {
        let mut book = OrderBook::new();
        let price = Price::new(100).unwrap();
        let order = make_order(1, UserId::new(1), price, 1, Side::Sell, Quantity::new(10));
        let order_id = order.id();
        book.add_resting_order(order).unwrap();

        let level = book.asks.get(&price).unwrap();
        assert_eq!(book.asks.len(), 1);
        assert_eq!(level.price(), price);
        assert_eq!(level.front().unwrap().id(), order_id);
        assert_eq!(book.bids.len(), 0);
    }

    #[test]
    fn empty_book_has_no_best_bid_or_ask() {
        let book = OrderBook::new();
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn best_bid_is_highest_bid_price() {
        let mut book = OrderBook::new();
        let price1 = Price::new(95).unwrap();
        let price2 = Price::new(100).unwrap();
        let price3 = Price::new(105).unwrap();

        let order1 = make_order(1, UserId::new(1), price1, 1, Side::Buy, Quantity::new(10));
        let order2 = make_order(2, UserId::new(1), price2, 2, Side::Buy, Quantity::new(10));
        let order3 = make_order(3, UserId::new(1), price3, 3, Side::Buy, Quantity::new(10));

        book.add_resting_order(order1).unwrap();
        book.add_resting_order(order3).unwrap();
        book.add_resting_order(order2).unwrap();
        assert_eq!(book.best_bid(), Some(price3));
    }

    #[test]
    fn best_ask_is_lowest_ask_price() {
        let mut book = OrderBook::new();
        let price1 = Price::new(95).unwrap();
        let price2 = Price::new(100).unwrap();
        let price3 = Price::new(105).unwrap();

        let order1 = make_order(1, UserId::new(1), price1, 1, Side::Sell, Quantity::new(10));
        let order2 = make_order(2, UserId::new(1), price2, 2, Side::Sell, Quantity::new(10));
        let order3 = make_order(3, UserId::new(1), price3, 3, Side::Sell, Quantity::new(10));

        book.add_resting_order(order1).unwrap();
        book.add_resting_order(order3).unwrap();
        book.add_resting_order(order2).unwrap();
        assert_eq!(book.best_ask(), Some(price1));
    }

    #[test]
    fn buy_crosses_asks_at_or_below_its_limit() {
        let buy_price = Price::new(100).unwrap();
        let buy_order = make_order(
            2,
            UserId::new(1),
            buy_price,
            2,
            Side::Buy,
            Quantity::new(10),
        );
        assert!(OrderBook::crosses(&buy_order, Price::new(99).unwrap()));
        assert!(OrderBook::crosses(&buy_order, Price::new(100).unwrap()));
        assert!(!OrderBook::crosses(&buy_order, Price::new(101).unwrap()));
    }

    #[test]
    fn sell_crosses_bids_at_or_above_its_limit() {
        let ask_price = Price::new(100).unwrap();
        let ask_order = make_order(
            2,
            UserId::new(1),
            ask_price,
            2,
            Side::Sell,
            Quantity::new(10),
        );

        assert!(!OrderBook::crosses(&ask_order, Price::new(99).unwrap()));
        assert!(OrderBook::crosses(&ask_order, Price::new(100).unwrap()));
        assert!(OrderBook::crosses(&ask_order, Price::new(101).unwrap()));
    }

    #[test]
    fn trade_resting_sell() {
        let mut incoming_order = make_order(
            1,
            UserId::new(1),
            Price::new(105).unwrap(),
            2,
            Side::Buy,
            Quantity::new(3),
        );
        let mut resting_order = make_order(
            2,
            UserId::new(2),
            Price::new(100).unwrap(),
            1,
            Side::Sell,
            Quantity::new(5),
        );

        let trade = OrderBook::execute_trade(&mut incoming_order, &mut resting_order);

        assert_eq!(trade.maker_user_id(), resting_order.user_id());
        assert_eq!(trade.taker_user_id(), incoming_order.user_id());
        assert_eq!(trade.maker_order_id(), resting_order.id());
        assert_eq!(trade.taker_order_id(), incoming_order.id());
        assert_eq!(trade.taker_side(), incoming_order.side());
        assert_eq!(trade.price(), resting_order.limit_price().unwrap());
        assert_eq!(trade.quantity(), Quantity::new(3));
        assert_eq!(incoming_order.remaining_quantity(), Quantity::new(0));
        assert_eq!(resting_order.remaining_quantity(), Quantity::new(2));
        assert!(incoming_order.is_filled());
        assert!(!resting_order.is_filled());
    }

    #[test]
    fn trade_resting_buy() {
        let mut incoming_order = make_order(
            1,
            UserId::new(1),
            Price::new(95).unwrap(),
            2,
            Side::Sell,
            Quantity::new(5),
        );
        let mut resting_order = make_order(
            2,
            UserId::new(2),
            Price::new(100).unwrap(),
            1,
            Side::Buy,
            Quantity::new(2),
        );

        let trade = OrderBook::execute_trade(&mut incoming_order, &mut resting_order);

        assert_eq!(trade.maker_user_id(), resting_order.user_id());
        assert_eq!(trade.taker_user_id(), incoming_order.user_id());
        assert_eq!(trade.maker_order_id(), resting_order.id());
        assert_eq!(trade.taker_order_id(), incoming_order.id());
        assert_eq!(trade.taker_side(), incoming_order.side());
        assert_eq!(trade.price(), Price::new(100).unwrap());
        assert_eq!(trade.price(), resting_order.limit_price().unwrap());
        assert_eq!(trade.quantity(), Quantity::new(2));
        assert_eq!(incoming_order.remaining_quantity(), Quantity::new(3));
        assert_eq!(resting_order.remaining_quantity(), Quantity::new(0));
        assert!(!incoming_order.is_filled());
        assert!(resting_order.is_filled());
    }

    #[test]
    fn incoming_buy_fills_part_of_best_ask() {
        let mut order_book = OrderBook::new();

        let resting_price = Price::new(100).unwrap();
        let resting_order = make_order(
            1,
            UserId::new(1),
            resting_price,
            1,
            Side::Sell,
            Quantity::new(5),
        );
        let resting_order_id = resting_order.id();

        let initial_result = order_book.place(resting_order);
        assert!(initial_result.trades().is_empty());

        let incoming_order = make_order(
            2,
            UserId::new(2),
            Price::new(105).unwrap(),
            2,
            Side::Buy,
            Quantity::new(3),
        );
        let incoming_order_id = incoming_order.id();

        let result = order_book.place(incoming_order);
        assert_eq!(result.trades().len(), 1);
        let trade = &result.trades()[0];

        assert_eq!(trade.maker_order_id(), resting_order_id);
        assert_eq!(trade.taker_order_id(), incoming_order_id);
        assert_eq!(trade.price(), resting_price);
        assert_eq!(trade.quantity(), Quantity::new(3));

        let level = order_book.asks.get(&resting_price).unwrap();
        let maker = level.front().unwrap();

        assert_eq!(maker.id(), resting_order_id);
        assert_eq!(maker.remaining_quantity(), Quantity::new(2));
        assert!(!maker.is_filled());

        assert_eq!(order_book.best_ask(), Some(resting_price));
        assert_eq!(order_book.best_bid(), None);
    }

    #[test]
    fn incoming_buy_fills_best_ask_and_rests_remainder() {
        let mut order_book = OrderBook::new();

        let resting_price = Price::new(100).unwrap();
        let resting_order = make_order(
            1,
            UserId::new(1),
            resting_price,
            1,
            Side::Sell,
            Quantity::new(3),
        );
        let resting_order_id = resting_order.id();

        let initial_result = order_book.place(resting_order);
        assert!(initial_result.trades().is_empty());

        let incoming_price = Price::new(105).unwrap();
        let incoming_order = make_order(
            2,
            UserId::new(2),
            incoming_price,
            2,
            Side::Buy,
            Quantity::new(5),
        );
        let incoming_order_id = incoming_order.id();

        let result = order_book.place(incoming_order);
        assert_eq!(result.trades().len(), 1);
        let trade = &result.trades()[0];

        assert_eq!(trade.maker_order_id(), resting_order_id);
        assert_eq!(trade.taker_order_id(), incoming_order_id);
        assert_eq!(trade.price(), resting_price);
        assert_eq!(trade.quantity(), Quantity::new(3));

        let level = order_book.bids.get(&incoming_price).unwrap();
        let taker = level.front().unwrap();

        assert_eq!(taker.id(), incoming_order_id);
        assert_eq!(taker.remaining_quantity(), Quantity::new(2));
        assert!(!taker.is_filled());

        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), Some(incoming_price));
        assert!(!order_book.asks.contains_key(&resting_price));
    }

    #[test]
    fn incoming_buy_matches_fifo_across_ask_levels() {
        let mut order_book = OrderBook::new();
        let order1_price = Price::new(100).unwrap();
        let order1 = make_order(
            1,
            UserId::new(1),
            order1_price,
            1,
            Side::Sell,
            Quantity::new(1),
        );

        let order2_price = Price::new(100).unwrap();
        let order2 = make_order(
            2,
            UserId::new(2),
            order2_price,
            2,
            Side::Sell,
            Quantity::new(2),
        );

        let order3_price = Price::new(101).unwrap();
        let order3 = make_order(
            3,
            UserId::new(3),
            order3_price,
            3,
            Side::Sell,
            Quantity::new(3),
        );

        let order4_price = Price::new(101).unwrap();
        let order4 = make_order(
            4,
            UserId::new(4),
            order4_price,
            4,
            Side::Buy,
            Quantity::new(4),
        );

        let initial_result1 = order_book.place(order1);
        assert!(initial_result1.trades().is_empty());

        let initial_result2 = order_book.place(order2);
        assert!(initial_result2.trades().is_empty());

        let initial_result3 = order_book.place(order3);
        assert!(initial_result3.trades().is_empty());

        let result = order_book.place(order4);
        assert_eq!(result.trades().len(), 3);

        let alice_bob_trade = &result.trades()[0];
        let carol_bob_trade = &result.trades()[1];
        let dave_bob_trade = &result.trades()[2];

        assert_eq!(alice_bob_trade.maker_user_id(), UserId::new(1));
        assert_eq!(alice_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(alice_bob_trade.quantity(), Quantity::new(1));
        assert_eq!(alice_bob_trade.maker_order_id(), OrderId::new(1));
        assert_eq!(alice_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(alice_bob_trade.price(), order1_price);

        assert_eq!(carol_bob_trade.maker_user_id(), UserId::new(2));
        assert_eq!(carol_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(carol_bob_trade.quantity(), Quantity::new(2));
        assert_eq!(carol_bob_trade.maker_order_id(), OrderId::new(2));
        assert_eq!(carol_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(carol_bob_trade.price(), order2_price);

        assert_eq!(dave_bob_trade.maker_user_id(), UserId::new(3));
        assert_eq!(dave_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(dave_bob_trade.quantity(), Quantity::new(1));
        assert_eq!(dave_bob_trade.maker_order_id(), OrderId::new(3));
        assert_eq!(dave_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(dave_bob_trade.price(), order3_price);

        assert!(!order_book.asks.contains_key(&order1_price));
        let ask_level = order_book.asks.get(&order3_price).unwrap();
        let maker = ask_level.front().unwrap();

        assert_eq!(maker.remaining_quantity(), Quantity::new(2));
        assert_eq!(maker.id(), OrderId::new(3));
        assert_eq!(order_book.best_ask(), Some(order3_price));
        assert_eq!(order_book.best_bid(), None);
    }

    #[test]
    fn non_crossing_buy_rests_without_changing_ask() {
        let mut order_book = OrderBook::new();

        let resting_price = Price::new(100).unwrap();
        let resting_order = make_order(
            1,
            UserId::new(1),
            resting_price,
            1,
            Side::Sell,
            Quantity::new(5),
        );
        let resting_order_id = resting_order.id();

        let incoming_price = Price::new(99).unwrap();
        let incoming_order = make_order(
            2,
            UserId::new(2),
            incoming_price,
            2,
            Side::Buy,
            Quantity::new(3),
        );
        let incoming_order_id = incoming_order.id();

        let resting_result = order_book.place(resting_order);
        assert_eq!(resting_result.trades().len(), 0);

        let incoming_result = order_book.place(incoming_order);
        assert_eq!(incoming_result.trades().len(), 0);

        assert_eq!(order_book.best_ask(), Some(resting_price));
        assert_eq!(order_book.best_bid(), Some(incoming_price));

        let ask_level = order_book.asks.get(&resting_price).unwrap();
        assert_eq!(ask_level.front().unwrap().id(), resting_order_id);
        assert_eq!(
            ask_level.front().unwrap().remaining_quantity(),
            Quantity::new(5)
        );

        let bid_level = order_book.bids.get(&incoming_price).unwrap();
        assert_eq!(bid_level.front().unwrap().id(), incoming_order_id);
        assert_eq!(
            bid_level.front().unwrap().remaining_quantity(),
            Quantity::new(3)
        );
    }

    #[test]
    fn incoming_sell_matches_fifo_across_bid_levels() {
        let mut order_book = OrderBook::new();

        let alice_price = Price::new(101).unwrap();
        let alice_order = make_order(
            1,
            UserId::new(1),
            alice_price,
            1,
            Side::Buy,
            Quantity::new(1),
        );

        let carol_price = Price::new(101).unwrap();
        let carol_order = make_order(
            2,
            UserId::new(2),
            carol_price,
            2,
            Side::Buy,
            Quantity::new(2),
        );

        let dave_price = Price::new(100).unwrap();
        let dave_order = make_order(
            3,
            UserId::new(3),
            dave_price,
            3,
            Side::Buy,
            Quantity::new(3),
        );

        let bob_price = Price::new(100).unwrap();
        let bob_order = make_order(
            4,
            UserId::new(4),
            bob_price,
            4,
            Side::Sell,
            Quantity::new(4),
        );

        let result1 = order_book.place(alice_order);
        assert_eq!(result1.trades().len(), 0);

        let result2 = order_book.place(carol_order);
        assert_eq!(result2.trades().len(), 0);

        let result3 = order_book.place(dave_order);
        assert_eq!(result3.trades().len(), 0);

        let result = order_book.place(bob_order);
        assert_eq!(result.trades().len(), 3);

        let alice_bob_trade = &result.trades()[0];
        let carol_bob_trade = &result.trades()[1];
        let dave_bob_trade = &result.trades()[2];

        assert_eq!(alice_bob_trade.maker_user_id(), UserId::new(1));
        assert_eq!(alice_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(alice_bob_trade.quantity(), Quantity::new(1));
        assert_eq!(alice_bob_trade.maker_order_id(), OrderId::new(1));
        assert_eq!(alice_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(alice_bob_trade.price(), alice_price);

        assert_eq!(carol_bob_trade.maker_user_id(), UserId::new(2));
        assert_eq!(carol_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(carol_bob_trade.quantity(), Quantity::new(2));
        assert_eq!(carol_bob_trade.maker_order_id(), OrderId::new(2));
        assert_eq!(carol_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(carol_bob_trade.price(), carol_price);

        assert_eq!(dave_bob_trade.maker_user_id(), UserId::new(3));
        assert_eq!(dave_bob_trade.taker_user_id(), UserId::new(4));
        assert_eq!(dave_bob_trade.quantity(), Quantity::new(1));
        assert_eq!(dave_bob_trade.maker_order_id(), OrderId::new(3));
        assert_eq!(dave_bob_trade.taker_order_id(), OrderId::new(4));
        assert_eq!(dave_bob_trade.price(), dave_price);

        assert!(!order_book.bids.contains_key(&alice_price));
        let bid_level = order_book.bids.get(&dave_price).unwrap();
        let maker = bid_level.front().unwrap();

        assert_eq!(maker.remaining_quantity(), Quantity::new(2));
        assert_eq!(maker.id(), OrderId::new(3));
        assert_eq!(order_book.best_bid(), Some(dave_price));
        assert_eq!(order_book.best_ask(), None);
    }

    #[test]
    fn multiple_takers_consume_the_same_maker_order() {
        let mut order_book = OrderBook::new();

        let price = Price::new(100).unwrap();
        let alice_order = make_order(1, UserId::new(1), price, 1, Side::Sell, Quantity::new(5));

        let bob_order = make_order(2, UserId::new(2), price, 2, Side::Buy, Quantity::new(2));

        let carol_order = make_order(3, UserId::new(3), price, 3, Side::Buy, Quantity::new(4));

        let result1 = order_book.place(alice_order);
        assert_eq!(result1.trades().len(), 0);

        let result2 = order_book.place(bob_order);
        assert_eq!(result2.trades().len(), 1);
        let trade2 = &result2.trades()[0];
        assert_eq!(trade2.maker_user_id(), UserId::new(1));
        assert_eq!(trade2.taker_user_id(), UserId::new(2));
        assert_eq!(trade2.price(), price);
        assert_eq!(trade2.quantity(), Quantity::new(2));

        assert!(!order_book.bids.contains_key(&price));
        let ask_level = order_book.asks.get(&price).unwrap();
        assert_eq!(
            ask_level.front().unwrap().remaining_quantity(),
            Quantity::new(3)
        );

        let result3 = order_book.place(carol_order);
        assert_eq!(result3.trades().len(), 1);
        let trade3 = &result3.trades()[0];
        assert_eq!(trade3.maker_user_id(), UserId::new(1));
        assert_eq!(trade3.taker_user_id(), UserId::new(3));
        assert_eq!(trade3.price(), price);
        assert_eq!(trade3.quantity(), Quantity::new(3));

        assert!(!order_book.asks.contains_key(&price));
        let bid_level = order_book.bids.get(&price).unwrap();
        assert_eq!(
            bid_level.front().unwrap().remaining_quantity(),
            Quantity::new(1)
        );
        assert_eq!(bid_level.front().unwrap().id(), OrderId::new(3));
        assert_eq!(order_book.best_bid().unwrap(), Price::new(100).unwrap());
        assert_eq!(order_book.best_ask(), None);
    }

    #[test]
    fn remove_order_from_price_level() {
        let mut order_book = OrderBook::new();

        let price = Price::new(100).unwrap();
        let alice_order = make_order(1, UserId::new(1), price, 1, Side::Sell, Quantity::new(5));
        let bob_order = make_order(2, UserId::new(2), price, 2, Side::Sell, Quantity::new(2));
        let carol_order = make_order(3, UserId::new(3), price, 3, Side::Sell, Quantity::new(4));

        let _ = order_book.place(alice_order);
        let _ = order_book.place(bob_order);
        let _ = order_book.place(carol_order);

        let price_level = order_book.asks.get_mut(&price).unwrap();
        assert_eq!(price_level.len(), 3);
        let cancelled_order = price_level.remove(OrderId::new(2));
        assert_eq!(cancelled_order.unwrap().id(), OrderId::new(2));
        assert_eq!(price_level.len(), 2);

        assert_eq!(price_level.orders[0].id(), OrderId::new(1));
        assert_eq!(price_level.orders[1].id(), OrderId::new(3));
        assert!(price_level.remove(OrderId::new(999)).is_none());
    }

    #[test]
    fn cancelling_resting_order_removes_it_and_its_empty_level() {
        let mut order_book = OrderBook::new();
        let price = Price::new(100).unwrap();
        let order = make_order(1, UserId::new(1), price, 1, Side::Sell, Quantity::new(5));
        let order_id = order.id();
        assert!(order_book.place(order).trades().is_empty());
        assert_eq!(order_book.best_ask(), Some(price));

        let cancelled_order = order_book.cancel(order_id).unwrap();
        assert_eq!(cancelled_order.id(), order_id);
        assert_eq!(cancelled_order.remaining_quantity(), Quantity::new(5));
        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.cancel(order_id), Err(CancelError::OrderNotFound));
    }

    #[test]
    fn partially_filled_order_can_be_cancelled() {
        let mut order_book = OrderBook::new();
        let price = Price::new(100).unwrap();

        let alice_order = make_order(1, UserId::new(1), price, 1, Side::Sell, Quantity::new(5));
        let alice_order_id = alice_order.id();
        assert_eq!(order_book.place(alice_order).trades().len(), 0);
        let bob_order = make_order(2, UserId::new(2), price, 2, Side::Buy, Quantity::new(2));

        let result = order_book.place(bob_order);
        assert_eq!(result.trades().len(), 1);
        let trade = &result.trades()[0];
        assert_eq!(trade.quantity(), Quantity::new(2));

        let cancelled_order = order_book.cancel(alice_order_id).unwrap();

        assert_eq!(cancelled_order.original_quantity(), Quantity::new(5));
        assert_eq!(cancelled_order.remaining_quantity(), Quantity::new(3));
        assert_eq!(cancelled_order.id(), alice_order_id);

        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);
    }

    #[test]
    fn cancelling_bid_preserves_non_empty_price_level() {
        let mut order_book = OrderBook::new();
        let price = Price::new(100).unwrap();

        let alice_order = make_order(1, UserId::new(1), price, 1, Side::Buy, Quantity::new(5));
        let alice_order_id = alice_order.id();

        let bob_order = make_order(2, UserId::new(2), price, 2, Side::Buy, Quantity::new(6));
        let bob_order_id = bob_order.id();

        assert_eq!(order_book.place(alice_order).trades().len(), 0);
        assert_eq!(order_book.place(bob_order).trades().len(), 0);

        let cancelled_order = order_book.cancel(alice_order_id).unwrap();
        assert_eq!(cancelled_order.original_quantity(), Quantity::new(5));
        assert_eq!(cancelled_order.remaining_quantity(), Quantity::new(5));
        assert_eq!(cancelled_order.id(), alice_order_id);

        assert_eq!(order_book.best_bid(), Some(price));
        assert_eq!(order_book.best_ask(), None);

        let bids = order_book.bids.get(&price).unwrap();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids.front().unwrap().id(), bob_order_id);
    }

    #[test]
    fn fully_filled_market_buy_walks_asks() {
        let mut order_book = OrderBook::new();

        let alice_price = Price::new(100).unwrap();
        let alice_order = make_order(
            1,
            UserId::new(1),
            alice_price,
            1,
            Side::Sell,
            Quantity::new(1),
        );
        let alice_order_id = alice_order.id();

        let carol_price = Price::new(101).unwrap();
        let carol_order = make_order(
            2,
            UserId::new(2),
            carol_price,
            2,
            Side::Sell,
            Quantity::new(2),
        );
        let carol_order_id = carol_order.id();

        let bob_order = Order::new(
            OrderId::new(3),
            UserId::new(3),
            Side::Buy,
            OrderKind::Market,
            Quantity::new(3),
            SequenceNumber::new(3),
        )
        .unwrap();
        let bob_order_id = bob_order.id();

        assert_eq!(order_book.place(alice_order).trades().len(), 0);
        assert_eq!(order_book.place(carol_order).trades().len(), 0);

        let result = order_book.place(bob_order);
        assert_eq!(result.trades().len(), 2);

        let trade1 = &result.trades()[0];
        assert_eq!(trade1.price(), alice_price);
        assert_eq!(trade1.maker_user_id(), UserId::new(1));
        assert_eq!(trade1.maker_order_id(), alice_order_id);
        assert_eq!(trade1.taker_user_id(), UserId::new(3));
        assert_eq!(trade1.taker_order_id(), bob_order_id);
        assert_eq!(trade1.quantity(), Quantity::new(1));

        let trade2 = &result.trades()[1];
        assert_eq!(trade2.price(), carol_price);
        assert_eq!(trade2.maker_user_id(), UserId::new(2));
        assert_eq!(trade2.maker_order_id(), carol_order_id);
        assert_eq!(trade2.quantity(), Quantity::new(2));
        assert_eq!(trade2.taker_user_id(), UserId::new(3));
        assert_eq!(trade2.taker_order_id(), bob_order_id);

        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);

        assert_eq!(result.unfilled_quantity(), Quantity::new(0));
    }

    #[test]
    fn partially_filled_market_buy_does_not_rest() {
        let mut order_book = OrderBook::new();

        let alice_price = Price::new(100).unwrap();
        let alice_order = make_order(
            1,
            UserId::new(1),
            alice_price,
            1,
            Side::Sell,
            Quantity::new(1),
        );

        let carol_price = Price::new(101).unwrap();
        let carol_order = make_order(
            2,
            UserId::new(2),
            carol_price,
            2,
            Side::Sell,
            Quantity::new(2),
        );

        let bob_order = Order::new(
            OrderId::new(3),
            UserId::new(3),
            Side::Buy,
            OrderKind::Market,
            Quantity::new(4),
            SequenceNumber::new(3),
        )
        .unwrap();

        assert_eq!(order_book.place(alice_order).trades().len(), 0);
        assert_eq!(order_book.place(carol_order).trades().len(), 0);

        let result = order_book.place(bob_order);
        assert_eq!(result.trades().len(), 2);

        let trade1 = &result.trades()[0];
        assert_eq!(trade1.price(), alice_price);
        assert_eq!(trade1.quantity(), Quantity::new(1));

        let trade2 = &result.trades()[1];
        assert_eq!(trade2.price(), carol_price);
        assert_eq!(trade2.quantity(), Quantity::new(2));

        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);
        assert_eq!(result.unfilled_quantity(), Quantity::new(1));
    }

    #[test]
    fn partially_filled_market_sell_does_not_rest() {
        let mut order_book = OrderBook::new();

        let alice_price = Price::new(101).unwrap();
        let alice_order = make_order(
            1,
            UserId::new(1),
            alice_price,
            1,
            Side::Buy,
            Quantity::new(1),
        );
        let alice_order_id = alice_order.id();

        let carol_price = Price::new(100).unwrap();
        let carol_order = make_order(
            2,
            UserId::new(2),
            carol_price,
            2,
            Side::Buy,
            Quantity::new(2),
        );
        let carol_order_id = carol_order.id();

        let bob_order = Order::new(
            OrderId::new(3),
            UserId::new(3),
            Side::Sell,
            OrderKind::Market,
            Quantity::new(4),
            SequenceNumber::new(3),
        )
        .unwrap();
        let bob_order_id = bob_order.id();

        assert_eq!(order_book.place(alice_order).trades().len(), 0);
        assert_eq!(order_book.place(carol_order).trades().len(), 0);

        let result = order_book.place(bob_order);
        assert_eq!(result.trades().len(), 2);

        let trade1 = &result.trades()[0];
        assert_eq!(trade1.price(), alice_price);
        assert_eq!(trade1.quantity(), Quantity::new(1));
        assert_eq!(trade1.maker_user_id(), UserId::new(1));
        assert_eq!(trade1.maker_order_id(), alice_order_id);
        assert_eq!(trade1.taker_user_id(), UserId::new(3));
        assert_eq!(trade1.taker_order_id(), bob_order_id);

        let trade2 = &result.trades()[1];
        assert_eq!(trade2.price(), carol_price);
        assert_eq!(trade2.quantity(), Quantity::new(2));
        assert_eq!(trade2.maker_user_id(), UserId::new(2));
        assert_eq!(trade2.maker_order_id(), carol_order_id);
        assert_eq!(trade2.taker_user_id(), UserId::new(3));
        assert_eq!(trade2.taker_order_id(), bob_order_id);

        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);
        assert_eq!(result.unfilled_quantity(), Quantity::new(1));
    }

    #[test]
    fn market_order_with_no_liquidity_expires_entirely() {
        let mut order_book = OrderBook::new();

        let bob_order = Order::new(
            OrderId::new(3),
            UserId::new(3),
            Side::Sell,
            OrderKind::Market,
            Quantity::new(4),
            SequenceNumber::new(3),
        )
        .unwrap();

        let result = order_book.place(bob_order);
        assert!(result.trades().is_empty());
        assert_eq!(order_book.best_ask(), None);
        assert_eq!(order_book.best_bid(), None);
        assert_eq!(result.unfilled_quantity(), Quantity::new(4));
    }
}
