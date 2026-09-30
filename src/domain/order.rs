use crate::domain::primitives::{OrderId, Price, Quantity, SequenceNumber, UserId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderKind {
    Limit { price: Price },
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    PendingTrigger,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderStateChange {
    order_id: OrderId,
    status: OrderStatus,
    remaining_quantity: Quantity,
    sequence: Option<SequenceNumber>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    id: OrderId,
    user_id: UserId,
    side: Side,
    kind: OrderKind,
    original_quantity: Quantity,
    remaining_quantity: Quantity,
    sequence: SequenceNumber,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OrderError {
    ZeroQuantity,
    ExceedsRemaining,
    ZeroRemainingQuantity,
    RemainingExceedsOriginal,
    ZeroSequence,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

impl OrderStateChange {
    pub(crate) fn new(
        order_id: OrderId,
        status: OrderStatus,
        remaining_quantity: Quantity,
        sequence: Option<SequenceNumber>,
    ) -> Self {
        Self {
            order_id,
            status,
            remaining_quantity,
            sequence,
        }
    }

    pub fn order_id(&self) -> OrderId {
        self.order_id
    }

    pub fn status(&self) -> OrderStatus {
        self.status
    }

    pub fn remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    pub fn sequence(&self) -> Option<SequenceNumber> {
        self.sequence
    }
}

impl Order {
    pub fn new(
        id: OrderId,
        user_id: UserId,
        side: Side,
        kind: OrderKind,
        quantity: Quantity,
        sequence: SequenceNumber,
    ) -> Result<Order, OrderError> {
        if quantity.is_zero() {
            return Err(OrderError::ZeroQuantity);
        }

        Ok(Order {
            id,
            user_id,
            side,
            kind,
            original_quantity: quantity,
            remaining_quantity: quantity,
            sequence,
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

    pub fn limit_price(&self) -> Option<Price> {
        match self.kind {
            OrderKind::Limit { price } => Some(price),
            OrderKind::Market => None,
        }
    }

    pub fn original_quantity(&self) -> Quantity {
        self.original_quantity
    }

    pub fn remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    pub fn sequence(&self) -> SequenceNumber {
        self.sequence
    }

    pub fn kind(&self) -> OrderKind {
        self.kind
    }

    pub fn fill(&mut self, quantity: Quantity) -> Result<(), OrderError> {
        if quantity.is_zero() {
            return Err(OrderError::ZeroQuantity);
        }

        match self.remaining_quantity.checked_sub(quantity) {
            Some(new_remaining) => {
                self.remaining_quantity = new_remaining;
                Ok(())
            }
            None => Err(OrderError::ExceedsRemaining),
        }
    }

    pub fn is_filled(&self) -> bool {
        self.remaining_quantity.is_zero()
    }

    pub(crate) fn state_change_after_matching(&self) -> OrderStateChange {
        let status = if self.is_filled() {
            OrderStatus::Filled
        } else {
            match self.kind() {
                OrderKind::Limit { .. }
                    if self.remaining_quantity() == self.original_quantity() =>
                {
                    OrderStatus::Open
                }
                OrderKind::Limit { .. } => OrderStatus::PartiallyFilled,
                OrderKind::Market => OrderStatus::Expired,
            }
        };

        OrderStateChange::new(
            self.id(),
            status,
            self.remaining_quantity(),
            Some(self.sequence()),
        )
    }

    pub(crate) fn restore_active(
        id: OrderId,
        user_id: UserId,
        side: Side,
        kind: OrderKind,
        original_quantity: Quantity,
        remaining_quantity: Quantity,
        sequence: SequenceNumber,
    ) -> Result<Self, OrderError> {
        if original_quantity.is_zero() {
            return Err(OrderError::ZeroQuantity);
        }

        if remaining_quantity.is_zero() {
            return Err(OrderError::ZeroRemainingQuantity);
        }

        if remaining_quantity > original_quantity {
            return Err(OrderError::RemainingExceedsOriginal);
        }

        if sequence.value() == 0 {
            return Err(OrderError::ZeroSequence);
        }

        Ok(Self {
            id,
            user_id,
            side,
            kind,
            original_quantity,
            remaining_quantity,
            sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_opposite_is_sell() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
    }

    #[test]
    fn double_opposite_is_original() {
        assert_eq!(Side::Buy.opposite().opposite(), Side::Buy);
    }

    #[test]
    fn zero_quantity_order_is_rejected() {
        let order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(0),
            SequenceNumber::new(1),
        );
        assert_eq!(order, Err(OrderError::ZeroQuantity));
    }

    #[test]
    fn new_order_preserves_its_fields_and_starts_unfilled() {
        let new_order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(20),
            SequenceNumber::new(1),
        );
        let order = new_order.unwrap();
        assert_eq!(order.id(), OrderId::new(1));
        assert_eq!(order.user_id(), UserId::new(1));
        assert_eq!(order.side(), Side::Buy);
        assert_eq!(order.limit_price(), Some(Price::new(100).unwrap()));
        assert_eq!(order.original_quantity(), Quantity::new(20));
        assert_eq!(order.remaining_quantity(), Quantity::new(20));
        assert_eq!(order.sequence(), SequenceNumber::new(1));
        assert!(!order.is_filled());
    }

    #[test]
    fn partial_fill_reduces_remaining_but_not_original() {
        let new_order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(20),
            SequenceNumber::new(1),
        );
        let mut order = new_order.unwrap();
        assert_eq!(order.fill(Quantity::new(5)), Ok(()));
        assert_eq!(order.original_quantity(), Quantity::new(20));
        assert_eq!(order.remaining_quantity(), Quantity::new(15));
        assert!(!order.is_filled());
    }

    #[test]
    fn exact_fill_reduces_remaining_to_zero() {
        let new_order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(20),
            SequenceNumber::new(1),
        );
        let mut order = new_order.unwrap();
        assert_eq!(order.fill(Quantity::new(20)), Ok(()));
        assert_eq!(order.remaining_quantity(), Quantity::new(0));
        assert!(order.is_filled());
    }

    #[test]
    fn overfill_is_rejected_and_leaves_order_unchanged() {
        let new_order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(20),
            SequenceNumber::new(1),
        );
        let mut order = new_order.unwrap();
        assert_eq!(
            order.fill(Quantity::new(25)),
            Err(OrderError::ExceedsRemaining)
        );
        assert_eq!(order.original_quantity(), Quantity::new(20));
        assert_eq!(order.remaining_quantity(), Quantity::new(20));
        assert!(!order.is_filled());
    }

    #[test]
    fn zero_fill_is_rejected_and_leaves_order_unchanged() {
        let new_order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Limit {
                price: Price::new(100).unwrap(),
            },
            Quantity::new(20),
            SequenceNumber::new(1),
        );
        let mut order = new_order.unwrap();
        assert_eq!(order.fill(Quantity::new(0)), Err(OrderError::ZeroQuantity));
        assert_eq!(order.original_quantity(), Quantity::new(20));
        assert_eq!(order.remaining_quantity(), Quantity::new(20));
        assert!(!order.is_filled());
    }

    #[test]
    fn new_market_order_has_no_limit_price() {
        let order = Order::new(
            OrderId::new(1),
            UserId::new(1),
            Side::Buy,
            OrderKind::Market,
            Quantity::new(20),
            SequenceNumber::new(1),
        )
        .unwrap();
        assert_eq!(order.limit_price(), None);
        assert_eq!(order.kind(), OrderKind::Market);
        assert_eq!(order.id(), OrderId::new(1));
        assert_eq!(order.user_id(), UserId::new(1));
        assert_eq!(order.side(), Side::Buy);
        assert_eq!(order.original_quantity(), Quantity::new(20));
        assert_eq!(order.remaining_quantity(), Quantity::new(20));
        assert_eq!(order.sequence(), SequenceNumber::new(1));
    }
}
