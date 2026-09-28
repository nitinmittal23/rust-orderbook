use crate::domain::{
    order::Side,
    primitives::{OrderId, Price},
    stop_order::StopLimitOrder,
};

use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopOrderBook {
    sell_stops: BTreeMap<Price, VecDeque<StopLimitOrder>>,
    buy_stops: BTreeMap<Price, VecDeque<StopLimitOrder>>,
}

impl StopOrderBook {
    pub fn add(&mut self, order: StopLimitOrder) {
        match order.side() {
            Side::Buy => {
                self.buy_stops
                    .entry(order.stop_price())
                    .or_default()
                    .push_back(order);
            }
            Side::Sell => {
                self.sell_stops
                    .entry(order.stop_price())
                    .or_default()
                    .push_back(order);
            }
        }
    }

    pub fn take_triggered(&mut self, trade_price: Price) -> Vec<StopLimitOrder> {
        let triggered_sell_levels = self.sell_stops.split_off(&trade_price);

        let triggered_buy_prices: Vec<Price> = self
            .buy_stops
            .range(..=trade_price)
            .map(|(price, _)| *price)
            .collect();

        let mut triggered_orders = Vec::new();

        for price in triggered_buy_prices {
            if let Some(orders) = self.buy_stops.remove(&price) {
                triggered_orders.extend(orders);
            }
        }

        triggered_orders.extend(
            triggered_sell_levels
                .into_iter()
                .rev()
                .flat_map(|(_, orders)| orders),
        );

        triggered_orders
    }

    fn cancel_from(
        stops: &mut BTreeMap<Price, VecDeque<StopLimitOrder>>,
        order_id: OrderId,
    ) -> Option<StopLimitOrder> {
        let location = stops.iter().find_map(|(price, orders)| {
            orders
                .iter()
                .position(|order| order.id() == order_id)
                .map(|position| (*price, position))
        });

        let (price, position) = location?;

        let orders = stops.get_mut(&price)?;
        let removed_order = orders.remove(position);
        let level_is_empty = orders.is_empty();

        if level_is_empty {
            stops.remove(&price);
        }

        removed_order
    }

    pub fn cancel(&mut self, order_id: OrderId) -> Option<StopLimitOrder> {
        if let Some(order) = Self::cancel_from(&mut self.buy_stops, order_id) {
            return Some(order);
        }

        Self::cancel_from(&mut self.sell_stops, order_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::primitives::{Quantity, UserId};

    fn make_stop(id: u64, side: Side, stop_price: u128) -> StopLimitOrder {
        StopLimitOrder::new(
            OrderId::new(id),
            UserId::new(1),
            side,
            Price::new(stop_price).unwrap(),
            Price::new(stop_price).unwrap(),
            Quantity::new(100),
        )
        .unwrap()
    }

    #[test]
    fn added_stop_remains_pending_above_trigger_price() {
        let mut book = StopOrderBook::default();
        book.add(make_stop(1, Side::Sell, 100));

        let triggered_above = book.take_triggered(Price::new(101).unwrap());
        assert!(triggered_above.is_empty());

        let triggered_at_stop = book.take_triggered(Price::new(100).unwrap());
        assert_eq!(triggered_at_stop.len(), 1);
        assert_eq!(triggered_at_stop[0].id(), OrderId::new(1));
    }

    #[test]
    fn triggering_multiple_levels_preserves_price_priority_and_fifo() {
        let mut book = StopOrderBook::default();
        book.add(make_stop(1, Side::Sell, 2990));
        book.add(make_stop(2, Side::Sell, 2980));
        book.add(make_stop(3, Side::Sell, 2990));
        book.add(make_stop(4, Side::Sell, 3000));

        let triggered = book.take_triggered(Price::new(2985).unwrap());
        assert_eq!(triggered.len(), 3);
        assert_eq!(triggered[0].id(), OrderId::new(4));
        assert_eq!(triggered[1].id(), OrderId::new(1));
        assert_eq!(triggered[2].id(), OrderId::new(3));

        let later_triggered = book.take_triggered(Price::new(2980).unwrap());
        assert_eq!(later_triggered.len(), 1);
        assert_eq!(later_triggered[0].id(), OrderId::new(2));
    }

    #[test]
    fn cancellation_removes_only_requested_stop_and_deletes_empty_level() {
        let mut book = StopOrderBook::default();
        book.add(make_stop(1, Side::Sell, 100));
        book.add(make_stop(2, Side::Sell, 100));

        let cancelled_order = book.cancel(OrderId::new(1)).unwrap();
        assert_eq!(cancelled_order.id(), OrderId::new(1));

        let level = book.sell_stops.get(&Price::new(100).unwrap()).unwrap();
        assert_eq!(level.len(), 1);
        assert_eq!(level[0].id(), OrderId::new(2));

        let cancelled_order_later = book.cancel(OrderId::new(2)).unwrap();
        assert_eq!(cancelled_order_later.id(), OrderId::new(2));

        assert!(!book.sell_stops.contains_key(&Price::new(100).unwrap()));

        assert_eq!(book.cancel(OrderId::new(999)), None);
    }

    #[test]
    fn triggering_buy_stops_preserves_price_priority_and_fifo() {
        let mut book = StopOrderBook::default();
        book.add(make_stop(1, Side::Buy, 2990));
        book.add(make_stop(2, Side::Buy, 2980));
        book.add(make_stop(3, Side::Buy, 2990));
        book.add(make_stop(4, Side::Buy, 3000));

        let triggered = book.take_triggered(Price::new(2985).unwrap());
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id(), OrderId::new(2));

        let later_triggered = book.take_triggered(Price::new(2995).unwrap());
        assert_eq!(later_triggered.len(), 2);
        assert_eq!(later_triggered[0].id(), OrderId::new(1));
        assert_eq!(later_triggered[1].id(), OrderId::new(3));

        let finally_triggered = book.take_triggered(Price::new(3000).unwrap());
        assert_eq!(finally_triggered.len(), 1);
        assert_eq!(finally_triggered[0].id(), OrderId::new(4));
    }

    #[test]
    fn pending_buy_stop_can_be_cancelled_and_empty_level_is_removed() {
        let mut book = StopOrderBook::default();
        book.add(make_stop(1, Side::Buy, 100));
        book.add(make_stop(2, Side::Buy, 100));

        let cancelled_order = book.cancel(OrderId::new(1)).unwrap();
        assert_eq!(cancelled_order.id(), OrderId::new(1));

        let level = book.buy_stops.get(&Price::new(100).unwrap()).unwrap();
        assert_eq!(level.len(), 1);
        assert_eq!(level[0].id(), OrderId::new(2));

        let cancelled_order_later = book.cancel(OrderId::new(2)).unwrap();
        assert_eq!(cancelled_order_later.id(), OrderId::new(2));

        assert!(!book.buy_stops.contains_key(&Price::new(100).unwrap()));

        assert_eq!(book.cancel(OrderId::new(999)), None);
    }
}
