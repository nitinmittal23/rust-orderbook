use sqlx::types::BigDecimal;

use crate::accounting::ledger::{BalanceDelta, BalanceMovementReason};

pub(crate) fn delta_to_big_decimal(delta: BalanceDelta) -> BigDecimal {
    match delta {
        BalanceDelta::Increase(amount) => BigDecimal::from(amount.value()),
        BalanceDelta::Decrease(amount) => -BigDecimal::from(amount.value()),
        BalanceDelta::Unchanged => BigDecimal::from(0),
    }
}

pub(crate) fn movement_type(reason: BalanceMovementReason) -> &'static str {
    match reason {
        BalanceMovementReason::Deposit => "DEPOSIT",
        BalanceMovementReason::OrderLock { .. } => "ORDER_LOCK",
        BalanceMovementReason::OrderUnlock { .. } => "ORDER_UNLOCK",
        BalanceMovementReason::TradeSettlement { .. } => "TRADE_SETTLEMENT",
        BalanceMovementReason::AdminAdjustment => "ADMIN_ADJUSTMENT",
    }
}
