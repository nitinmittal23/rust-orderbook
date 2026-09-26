#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Price(u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quantity(u128);

#[derive(Debug, PartialEq, Eq)]
pub enum PriceError {
    Zero,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SequenceNumber(u64);

impl Price {
    pub fn new(price: u128) -> Result<Self, PriceError> {
        if price == 0 {
            Err(PriceError::Zero)
        } else {
            Ok(Price(price))
        }
    }

    pub fn value(&self) -> u128 {
        self.0
    }
}

impl Quantity {
    pub fn new(quantity: u128) -> Self {
        Quantity(quantity)
    }

    pub fn value(&self) -> u128 {
        self.0
    }

    pub fn checked_sub(self, other: Quantity) -> Option<Quantity> {
        self.0.checked_sub(other.0).map(Quantity)
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl OrderId {
    pub fn new(id: u64) -> Self {
        OrderId(id)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl UserId {
    pub fn new(id: u64) -> Self {
        UserId(id)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl SequenceNumber {
    pub fn new(id: u64) -> Self {
        SequenceNumber(id)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_price_is_rejected() {
        assert_eq!(Price::new(0), Err(PriceError::Zero));
    }

    #[test]
    fn positive_price_is_accepted() {
        assert_eq!(Price::new(100), Ok(Price(100)));
    }

    #[test]
    fn prices_are_ordered_numerically() {
        let lower = Price::new(3000).unwrap();
        let higher = Price::new(5000).unwrap();
        assert!(lower < higher);
        assert!(higher > lower);
    }

    #[test]
    fn quantity_subtraction_succeeds() {
        let q1 = Quantity::new(10);
        let q2 = Quantity::new(5);
        assert_eq!(q1.checked_sub(q2), Some(Quantity::new(5)));
    }

    #[test]
    fn quantity_subtraction_refuses_underflow() {
        let q1 = Quantity::new(10);
        let q2 = Quantity::new(5);
        assert_eq!(q2.checked_sub(q1), None);
    }

    #[test]
    fn prices_sort_numerically() {
        let mut prices = vec![
            Price::new(5000).unwrap(),
            Price::new(4000).unwrap(),
            Price::new(4500).unwrap(),
        ];

        prices.sort();

        assert_eq!(
            prices,
            vec![
                Price::new(4000).unwrap(),
                Price::new(4500).unwrap(),
                Price::new(5000).unwrap()
            ]
        );
    }

    #[test]
    fn sequence_numbers_are_ordered_numerically() {
        let lower = SequenceNumber::new(3000);
        let higher = SequenceNumber::new(5000);
        assert!(lower < higher);
        assert!(higher > lower);
    }

    #[test]
    fn order_id_preserves_its_value() {
        let id = OrderId::new(42);
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn user_id_preserves_its_value() {
        let id = UserId::new(42);
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn zero_quantity_is_zero() {
        assert!(Quantity::new(0).is_zero());
    }

    #[test]
    fn positive_quantity_is_not_zero() {
        assert!(!Quantity::new(20).is_zero());
    }
}
