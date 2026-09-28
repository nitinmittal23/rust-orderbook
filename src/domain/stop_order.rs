use crate::domain::{
    order::{Order, OrderError, OrderKind, Side},
    primitives::{OrderId, Price, Quantity, SequenceNumber, UserId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopLimitOrder {
    id: OrderId,
    user_id: UserId,
    side: Side,
    stop_price: Price,
    limit_price: Price,
    quantity: Quantity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopOrderError {
    ZeroQuantity,
}

impl StopLimitOrder {
    pub fn new(
        id: OrderId,
        user_id: UserId,
        side: Side,
        stop_price: Price,
        limit_price: Price,
        quantity: Quantity,
    ) -> Result<Self, StopOrderError> {
        if quantity.is_zero() {
            return Err(StopOrderError::ZeroQuantity);
        }

        Ok(StopLimitOrder {
            id,
            user_id,
            side,
            stop_price,
            limit_price,
            quantity,
        })
    }

    pub fn id(&self) -> OrderId {
        self.id
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn stop_price(&self) -> Price {
        self.stop_price
    }

    pub fn limit_price(&self) -> Price {
        self.limit_price
    }

    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    pub fn is_triggered_by(&self, trade_price: Price) -> bool {
        match self.side {
            Side::Buy => trade_price >= self.stop_price,
            Side::Sell => trade_price <= self.stop_price,
        }
    }

    pub fn into_active_order(self, sequence: SequenceNumber) -> Result<Order, OrderError> {
        Order::new(
            self.id,
            self.user_id,
            self.side,
            OrderKind::Limit {
                price: self.limit_price,
            },
            self.quantity,
            sequence,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_limit_order_preserves_submission_details() {
        let order = StopLimitOrder::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Sell,
            Price::new(1).unwrap(),
            Price::new(1).unwrap(),
            Quantity::new(100),
        )
        .unwrap();

        assert_eq!(order.id(), OrderId::new(1));
        assert_eq!(order.user_id(), UserId::new(1));
        assert_eq!(order.side(), Side::Sell);
        assert_eq!(order.stop_price(), Price::new(1).unwrap());
        assert_eq!(order.limit_price(), Price::new(1).unwrap());
        assert_eq!(order.quantity(), Quantity::new(100));
    }

    #[test]
    fn zero_quantity_is_rejected() {
        assert_eq!(
            StopLimitOrder::new(
                OrderId::new(1),
                UserId::new(1),
                Side::Sell,
                Price::new(1).unwrap(),
                Price::new(1).unwrap(),
                Quantity::new(0),
            ),
            Err(StopOrderError::ZeroQuantity),
        );
    }

    #[test]
    fn sell_stop_triggers_at_or_below_stop_price() {
        let order = StopLimitOrder::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Sell,
            Price::new(100).unwrap(),
            Price::new(100).unwrap(),
            Quantity::new(100),
        )
        .unwrap();

        assert!(order.is_triggered_by(Price::new(99).unwrap()));
        assert!(order.is_triggered_by(Price::new(100).unwrap()));
        assert!(!order.is_triggered_by(Price::new(101).unwrap()));
    }

    #[test]
    fn triggered_stop_becomes_limit_sell_with_same_identity() {
        let order = StopLimitOrder::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Sell,
            Price::new(100).unwrap(),
            Price::new(100).unwrap(),
            Quantity::new(100),
        )
        .unwrap();

        let limit_order = order.into_active_order(SequenceNumber::new(4)).unwrap();

        assert_eq!(limit_order.id(), OrderId::new(1));
        assert_eq!(limit_order.user_id(), UserId::new(1));
        assert_eq!(limit_order.limit_price(), Some(Price::new(100).unwrap()));
        assert_eq!(limit_order.side(), Side::Sell);
        assert_eq!(limit_order.original_quantity(), Quantity::new(100));
        assert_eq!(limit_order.remaining_quantity(), Quantity::new(100));
        assert_eq!(limit_order.sequence(), SequenceNumber::new(4));
    }

    #[test]
    fn buy_stop_triggers_at_or_above_stop_price() {
        let order = StopLimitOrder::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            Price::new(100).unwrap(),
            Price::new(105).unwrap(),
            Quantity::new(100),
        )
        .unwrap();

        assert!(!order.is_triggered_by(Price::new(99).unwrap()));
        assert!(order.is_triggered_by(Price::new(100).unwrap()));
        assert!(order.is_triggered_by(Price::new(101).unwrap()));
    }

    #[test]
    fn triggered_buy_stop_becomes_limit_buy_with_same_identity() {
        let order = StopLimitOrder::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            Price::new(100).unwrap(),
            Price::new(105).unwrap(),
            Quantity::new(100),
        )
        .unwrap();

        let limit_order = order.into_active_order(SequenceNumber::new(4)).unwrap();

        assert_eq!(limit_order.id(), OrderId::new(1));
        assert_eq!(limit_order.user_id(), UserId::new(1));
        assert_eq!(limit_order.limit_price(), Some(Price::new(105).unwrap()));
        assert_eq!(limit_order.side(), Side::Buy);
        assert_eq!(limit_order.original_quantity(), Quantity::new(100));
        assert_eq!(limit_order.remaining_quantity(), Quantity::new(100));
        assert_eq!(limit_order.sequence(), SequenceNumber::new(4));
    }
}
