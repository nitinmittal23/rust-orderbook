use crate::domain::asset::AssetSymbol;
use crate::domain::primitives::{AssetAmount, OrderId, TradeSequenceNumber, UserId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Balance {
    available: AssetAmount,
    locked: AssetAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceMovementReason {
    Deposit,
    OrderLock {
        order_id: OrderId,
    },
    OrderUnlock {
        order_id: OrderId,
    },
    TradeSettlement {
        order_id: OrderId,
        trade_sequence: TradeSequenceNumber,
    },
    AdminAdjustment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceDelta {
    Increase(AssetAmount),
    Decrease(AssetAmount),
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceMovement {
    user_id: UserId,
    asset: AssetSymbol,
    available_delta: BalanceDelta,
    locked_delta: BalanceDelta,
    reason: BalanceMovementReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceSnapshot {
    user_id: UserId,
    asset: AssetSymbol,
    available: AssetAmount,
    locked: AssetAmount,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LedgerError {
    InsufficientAvailable,
    InsufficientLocked,
    Overflow,
    EmptyMovement,
    BalanceAlreadyRestored,
}

#[derive(Debug, Default, Clone)]
pub struct Ledger {
    balances: HashMap<UserId, HashMap<AssetSymbol, Balance>>,
}

impl BalanceSnapshot {
    pub(crate) fn new(
        user_id: UserId,
        asset: AssetSymbol,
        available: AssetAmount,
        locked: AssetAmount,
    ) -> Self {
        Self {
            user_id,
            asset,
            available,
            locked,
        }
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn asset(&self) -> &AssetSymbol {
        &self.asset
    }

    pub fn available(&self) -> AssetAmount {
        self.available
    }

    pub fn locked(&self) -> AssetAmount {
        self.locked
    }
}

impl BalanceMovement {
    pub(crate) fn new(
        user_id: UserId,
        asset: AssetSymbol,
        available_delta: BalanceDelta,
        locked_delta: BalanceDelta,
        reason: BalanceMovementReason,
    ) -> Self {
        Self {
            user_id,
            asset,
            available_delta,
            locked_delta,
            reason,
        }
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn asset(&self) -> &AssetSymbol {
        &self.asset
    }

    pub fn available_delta(&self) -> BalanceDelta {
        self.available_delta
    }

    pub fn locked_delta(&self) -> BalanceDelta {
        self.locked_delta
    }

    pub fn reason(&self) -> BalanceMovementReason {
        self.reason
    }
}

impl Balance {
    pub fn available(&self) -> AssetAmount {
        self.available
    }

    pub fn locked(&self) -> AssetAmount {
        self.locked
    }
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn restore_balance(
        &mut self,
        user_id: UserId,
        asset: AssetSymbol,
        available: AssetAmount,
        locked: AssetAmount,
    ) -> Result<(), LedgerError> {
        let user_balances = self.balances.entry(user_id).or_default();

        if user_balances.contains_key(&asset) {
            return Err(LedgerError::BalanceAlreadyRestored);
        }

        user_balances.insert(asset, Balance { available, locked });

        Ok(())
    }

    pub fn balance(&self, user_id: UserId, asset: &AssetSymbol) -> Balance {
        self.balances
            .get(&user_id)
            .and_then(|assets| assets.get(asset))
            .copied()
            .unwrap_or_default()
    }

    fn balance_mut(&mut self, user_id: UserId, asset: &AssetSymbol) -> &mut Balance {
        self.balances
            .entry(user_id)
            .or_default()
            .entry(asset.clone())
            .or_default()
    }

    pub fn deposit(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), LedgerError> {
        let balance = self.balance_mut(user_id, asset);
        let new_available_balance = balance
            .available
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        balance.available = new_available_balance;
        Ok(())
    }

    pub fn credit(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), LedgerError> {
        self.deposit(user_id, asset, amount)
    }

    pub fn lock(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), LedgerError> {
        let balance = self.balance_mut(user_id, asset);
        let new_available = balance
            .available
            .checked_sub(amount)
            .ok_or(LedgerError::InsufficientAvailable)?;
        let new_locked = balance
            .locked
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        balance.available = new_available;
        balance.locked = new_locked;
        Ok(())
    }

    pub fn unlock(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), LedgerError> {
        let balance = self.balance_mut(user_id, asset);
        let new_locked = balance
            .locked
            .checked_sub(amount)
            .ok_or(LedgerError::InsufficientLocked)?;
        let new_available = balance
            .available
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        balance.available = new_available;
        balance.locked = new_locked;
        Ok(())
    }

    pub fn consume_locked(
        &mut self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<(), LedgerError> {
        let balance = self.balance_mut(user_id, asset);
        let new_locked = balance
            .locked
            .checked_sub(amount)
            .ok_or(LedgerError::InsufficientLocked)?;
        balance.locked = new_locked;
        Ok(())
    }

    pub fn settle_trade(
        &mut self,
        buyer_id: UserId,
        seller_id: UserId,
        base_asset: &AssetSymbol,
        quote_asset: &AssetSymbol,
        base_amount: AssetAmount,
        quote_amount: AssetAmount,
        buyer_quote_refund: AssetAmount,
    ) -> Result<(), LedgerError> {
        let mut staged = self.clone();

        staged.consume_locked(buyer_id, quote_asset, quote_amount)?;
        staged.consume_locked(seller_id, base_asset, base_amount)?;
        staged.credit(buyer_id, base_asset, base_amount)?;
        staged.credit(seller_id, quote_asset, quote_amount)?;
        staged.unlock(buyer_id, quote_asset, buyer_quote_refund)?;

        *self = staged;
        Ok(())
    }

    fn apply_available_delta(
        current: AssetAmount,
        delta: BalanceDelta,
    ) -> Result<AssetAmount, LedgerError> {
        match delta {
            BalanceDelta::Increase(amount) => {
                current.checked_add(amount).ok_or(LedgerError::Overflow)
            }
            BalanceDelta::Decrease(amount) => current
                .checked_sub(amount)
                .ok_or(LedgerError::InsufficientAvailable),
            BalanceDelta::Unchanged => Ok(current),
        }
    }

    fn apply_locked_delta(
        current: AssetAmount,
        delta: BalanceDelta,
    ) -> Result<AssetAmount, LedgerError> {
        match delta {
            BalanceDelta::Increase(amount) => {
                current.checked_add(amount).ok_or(LedgerError::Overflow)
            }
            BalanceDelta::Decrease(amount) => current
                .checked_sub(amount)
                .ok_or(LedgerError::InsufficientLocked),
            BalanceDelta::Unchanged => Ok(current),
        }
    }

    fn apply_movement(&mut self, movement: &BalanceMovement) -> Result<(), LedgerError> {
        if movement.available_delta() == BalanceDelta::Unchanged
            && movement.locked_delta() == BalanceDelta::Unchanged
        {
            return Err(LedgerError::EmptyMovement);
        }

        let balance = self.balance_mut(movement.user_id(), movement.asset());

        let new_available =
            Self::apply_available_delta(balance.available, movement.available_delta())?;

        let new_locked = Self::apply_locked_delta(balance.locked, movement.locked_delta())?;

        balance.available = new_available;
        balance.locked = new_locked;

        Ok(())
    }

    pub(crate) fn apply_movements(
        &mut self,
        movements: &[BalanceMovement],
    ) -> Result<(), LedgerError> {
        let mut staged = self.clone();
        for movement in movements {
            staged.apply_movement(movement)?;
        }
        *self = staged;
        Ok(())
    }

    pub(crate) fn snapshots_for_movements(
        &self,
        movements: &[BalanceMovement],
    ) -> Vec<BalanceSnapshot> {
        let mut seen = HashSet::new();
        let mut snapshots = Vec::new();

        for movement in movements {
            let user_id = movement.user_id();
            let asset = movement.asset().clone();

            if seen.insert((user_id, asset.clone())) {
                let balance = self.balance(user_id, &asset);

                snapshots.push(BalanceSnapshot::new(
                    user_id,
                    asset,
                    balance.available(),
                    balance.locked(),
                ));
            }
        }

        snapshots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_balance(
        ledger: &Ledger,
        user_id: UserId,
        asset: &AssetSymbol,
        expected_available: u128,
        expected_locked: u128,
    ) {
        let balance = ledger.balance(user_id, asset);
        assert_eq!(balance.available(), AssetAmount::new(expected_available));
        assert_eq!(balance.locked(), AssetAmount::new(expected_locked));
    }

    #[test]
    fn ledger_moves_funds_through_lifecycle() {
        let mut ledger = Ledger::new();
        let alice = UserId::new(1);
        let usdc = AssetSymbol::new("USDC").unwrap();

        assert_balance(&ledger, alice, &usdc, 0, 0);

        ledger
            .deposit(alice, &usdc, AssetAmount::new(1_000))
            .unwrap();
        assert_balance(&ledger, alice, &usdc, 1_000, 0);

        ledger.lock(alice, &usdc, AssetAmount::new(600)).unwrap();
        assert_balance(&ledger, alice, &usdc, 400, 600);

        ledger.unlock(alice, &usdc, AssetAmount::new(200)).unwrap();
        assert_balance(&ledger, alice, &usdc, 600, 400);

        ledger
            .consume_locked(alice, &usdc, AssetAmount::new(200))
            .unwrap();
        assert_balance(&ledger, alice, &usdc, 600, 200);

        ledger.credit(alice, &usdc, AssetAmount::new(300)).unwrap();
        assert_balance(&ledger, alice, &usdc, 900, 200);
    }

    #[test]
    fn failed_operations_preserve_balance() {
        let mut ledger = Ledger::new();
        let alice = UserId::new(1);
        let usdc = AssetSymbol::new("USDC").unwrap();

        assert_balance(&ledger, alice, &usdc, 0, 0);

        ledger
            .deposit(alice, &usdc, AssetAmount::new(1_000))
            .unwrap();
        assert_balance(&ledger, alice, &usdc, 1_000, 0);

        ledger.lock(alice, &usdc, AssetAmount::new(600)).unwrap();
        assert_balance(&ledger, alice, &usdc, 400, 600);

        assert_eq!(
            ledger.lock(alice, &usdc, AssetAmount::new(500)),
            Err(LedgerError::InsufficientAvailable)
        );
        assert_balance(&ledger, alice, &usdc, 400, 600);

        assert_eq!(
            ledger.unlock(alice, &usdc, AssetAmount::new(700)),
            Err(LedgerError::InsufficientLocked)
        );
        assert_balance(&ledger, alice, &usdc, 400, 600);

        assert_eq!(
            ledger.consume_locked(alice, &usdc, AssetAmount::new(700)),
            Err(LedgerError::InsufficientLocked)
        );
        assert_balance(&ledger, alice, &usdc, 400, 600);
    }

    #[test]
    fn deposit_overflow_preserves_balance() {
        let mut ledger = Ledger::new();
        let alice = UserId::new(1);
        let usdc = AssetSymbol::new("USDC").unwrap();

        assert_balance(&ledger, alice, &usdc, 0, 0);

        ledger
            .deposit(alice, &usdc, AssetAmount::new(u128::MAX))
            .unwrap();
        let before = ledger.balance(alice, &usdc);

        assert_eq!(
            ledger.deposit(alice, &usdc, AssetAmount::new(1)),
            Err(LedgerError::Overflow)
        );
        assert_eq!(ledger.balance(alice, &usdc), before);
    }

    #[test]
    fn settle_trade_moves_locked_funds_between_users() {
        let mut ledger = Ledger::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);

        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        // Alice is the buyer: she deposits and locks 100 USDC.
        ledger.deposit(alice, &usdc, AssetAmount::new(100)).unwrap();
        ledger.lock(alice, &usdc, AssetAmount::new(100)).unwrap();

        // Bob is the seller: he deposits and locks 1 ETH.
        ledger.deposit(bob, &eth, AssetAmount::new(1)).unwrap();
        ledger.lock(bob, &eth, AssetAmount::new(1)).unwrap();

        ledger
            .settle_trade(
                alice,
                bob,
                &eth,
                &usdc,
                AssetAmount::new(1),   // Base amount: 1 ETH
                AssetAmount::new(100), // Quote amount: 100 USDC
                AssetAmount::new(0),   // No price-improvement refund
            )
            .unwrap();

        // Alice spent her locked USDC and received ETH.
        assert_balance(&ledger, alice, &usdc, 0, 0);
        assert_balance(&ledger, alice, &eth, 1, 0);

        // Bob spent his locked ETH and received USDC.
        assert_balance(&ledger, bob, &eth, 0, 0);
        assert_balance(&ledger, bob, &usdc, 100, 0);
    }

    #[test]
    fn settle_trade_refunds_buyer_price_improvement() {
        let mut ledger = Ledger::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);

        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        // Alice is the buyer: she deposits and locks 100 USDC.
        ledger.deposit(alice, &usdc, AssetAmount::new(101)).unwrap();
        ledger.lock(alice, &usdc, AssetAmount::new(101)).unwrap();

        // Bob is the seller: he deposits and locks 1 ETH.
        ledger.deposit(bob, &eth, AssetAmount::new(1)).unwrap();
        ledger.lock(bob, &eth, AssetAmount::new(1)).unwrap();

        ledger
            .settle_trade(
                alice,
                bob,
                &eth,
                &usdc,
                AssetAmount::new(1),   // Base amount: 1 ETH
                AssetAmount::new(100), // Quote amount: 100 USDC
                AssetAmount::new(1),   // price-improvement refund
            )
            .unwrap();

        // Alice spent her locked USDC and received ETH.
        assert_balance(&ledger, alice, &usdc, 1, 0);
        assert_balance(&ledger, alice, &eth, 1, 0);

        // Bob spent his locked ETH and received USDC.
        assert_balance(&ledger, bob, &eth, 0, 0);
        assert_balance(&ledger, bob, &usdc, 100, 0);
    }

    #[test]
    fn settle_trade_failure_leaves_ledger_unchanged() {
        let mut ledger = Ledger::new();

        let alice = UserId::new(1);
        let bob = UserId::new(2);

        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        // Alice is the buyer: she deposits and locks 100 USDC.
        ledger.deposit(alice, &usdc, AssetAmount::new(100)).unwrap();
        ledger.lock(alice, &usdc, AssetAmount::new(100)).unwrap();

        let result = ledger.settle_trade(
            alice,
            bob,
            &eth,
            &usdc,
            AssetAmount::new(1),   // Base amount: 1 ETH
            AssetAmount::new(100), // Quote amount: 100 USDC
            AssetAmount::new(1),   // No price-improvement refund
        );
        assert_eq!(result, Err(LedgerError::InsufficientLocked));

        assert_balance(&ledger, alice, &usdc, 0, 100);

        assert_balance(&ledger, alice, &eth, 0, 0);
        assert_balance(&ledger, bob, &eth, 0, 0);
        assert_balance(&ledger, bob, &usdc, 0, 0);
    }
}
