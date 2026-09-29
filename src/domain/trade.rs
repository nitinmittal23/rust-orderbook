use crate::domain::order::Side;
use crate::domain::primitives::{OrderId, Price, Quantity, TradeSequenceNumber, UserId};

#[derive(Debug, PartialEq, Eq)]
pub struct Trade {
    maker_order_id: OrderId,
    taker_order_id: OrderId,
    maker_user_id: UserId,
    taker_user_id: UserId,
    taker_side: Side,
    price: Price,
    quantity: Quantity,
    taker_limit_price: Option<Price>,
    sequence: Option<TradeSequenceNumber>,
}

impl Trade {
    pub(crate) fn new(
        maker_order_id: OrderId,
        taker_order_id: OrderId,
        maker_user_id: UserId,
        taker_user_id: UserId,
        taker_side: Side,
        price: Price,
        quantity: Quantity,
        taker_limit_price: Option<Price>,
    ) -> Self {
        Self {
            maker_order_id,
            taker_order_id,
            maker_user_id,
            taker_user_id,
            taker_side,
            price,
            quantity,
            taker_limit_price,
            sequence: None,
        }
    }

    pub fn maker_order_id(&self) -> OrderId {
        self.maker_order_id
    }

    pub fn taker_order_id(&self) -> OrderId {
        self.taker_order_id
    }

    pub fn maker_user_id(&self) -> UserId {
        self.maker_user_id
    }

    pub fn taker_user_id(&self) -> UserId {
        self.taker_user_id
    }

    pub fn taker_side(&self) -> Side {
        self.taker_side
    }

    pub fn price(&self) -> Price {
        self.price
    }

    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    pub fn taker_limit_price(&self) -> Option<Price> {
        self.taker_limit_price
    }

    pub fn sequence(&self) -> Option<TradeSequenceNumber> {
        self.sequence
    }

    pub(crate) fn assign_sequence(&mut self, sequence: TradeSequenceNumber) {
        assert!(
            self.sequence.is_none(),
            "trade sequence can only be assigned once"
        );
        self.sequence = Some(sequence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trade_preserves_its_execution_details() {
        let trade = Trade::new(
            OrderId::new(1),
            OrderId::new(2),
            UserId::new(3),
            UserId::new(4),
            Side::Buy,
            Price::new(4).unwrap(),
            Quantity::new(100),
            Some(Price::new(100).unwrap()),
        );

        assert_eq!(trade.maker_order_id(), OrderId::new(1));
        assert_eq!(trade.taker_order_id(), OrderId::new(2));
        assert_eq!(trade.maker_user_id(), UserId::new(3));
        assert_eq!(trade.taker_user_id(), UserId::new(4));
        assert_eq!(trade.taker_side(), Side::Buy);
        assert_eq!(trade.price(), Price::new(4).unwrap());
        assert_eq!(trade.quantity(), Quantity::new(100));
        assert_eq!(trade.taker_limit_price(), Some(Price::new(100).unwrap()));
        assert_eq!(trade.sequence(), None);
    }
}
