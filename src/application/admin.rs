use std::sync::Arc;

use sqlx::{PgPool, types::BigDecimal};
use tokio::sync::Mutex;

use crate::{
    application::balance_mapping::{delta_to_big_decimal, movement_type},
    domain::{
        asset::{Asset, AssetSymbol},
        pair::TradingPair,
        primitives::{AssetAmount, Price, Quantity, UserId},
    },
    exchange::{Exchange, ExchangeError},
    persistence::postgres::{
        assets::{self, AssetRecord},
        balance_movements::{self, BalanceMovementInsert},
        balances,
        deposits::{self, DepositInsert, DepositRecord},
        markets::{self, MarketRecord},
        users,
    },
};

use uuid::Uuid;

#[derive(Clone)]
pub struct AdminService {
    exchange: Arc<Mutex<Exchange>>,
    db: PgPool,
}

#[derive(Debug)]
pub enum AdminServiceError {
    Exchange(ExchangeError),
    Database(sqlx::Error),
    UnknownBaseAsset,
    UnknownQuoteAsset,
    BaseAssetDisabled,
    QuoteAssetDisabled,
    UnknownUser,
    UserDisabled,
    UnknownAsset,
    AssetDisabled,
    IdentifierOutOfRange,
    UnexpectedEngineOutput,
    InvalidAssetName,
    InvalidDepositReference,
    DepositReferenceConflict,
}

impl AdminService {
    pub fn new(exchange: Arc<Mutex<Exchange>>, db: PgPool) -> Self {
        Self { exchange, db }
    }

    pub async fn create_asset(
        &self,
        asset: Asset,
        name: &str,
    ) -> Result<AssetRecord, AdminServiceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminServiceError::InvalidAssetName);
        }

        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        staged_exchange
            .register_asset(asset.clone())
            .map_err(AdminServiceError::Exchange)?;

        let mut transaction = self.db.begin().await.map_err(AdminServiceError::Database)?;

        let record = assets::insert(transaction.as_mut(), &asset, name)
            .await
            .map_err(AdminServiceError::Database)?;

        transaction
            .commit()
            .await
            .map_err(AdminServiceError::Database)?;

        *live_exchange = staged_exchange;

        Ok(record)
    }

    pub async fn create_market(
        &self,
        pair: TradingPair,
        price_tick: Price,
        quantity_step: Quantity,
    ) -> Result<MarketRecord, AdminServiceError> {
        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();

        let mut transaction = self.db.begin().await.map_err(AdminServiceError::Database)?;

        let base = assets::find_by_symbol(transaction.as_mut(), pair.base())
            .await
            .map_err(AdminServiceError::Database)?
            .ok_or(AdminServiceError::UnknownBaseAsset)?;

        if !base.enabled {
            return Err(AdminServiceError::BaseAssetDisabled);
        }

        let quote = assets::find_by_symbol(transaction.as_mut(), pair.quote())
            .await
            .map_err(AdminServiceError::Database)?
            .ok_or(AdminServiceError::UnknownQuoteAsset)?;

        if !quote.enabled {
            return Err(AdminServiceError::QuoteAssetDisabled);
        }

        staged_exchange
            .create_market(pair, price_tick, quantity_step)
            .map_err(AdminServiceError::Exchange)?;

        let record = markets::insert(
            transaction.as_mut(),
            base.id,
            quote.id,
            price_tick,
            quantity_step,
        )
        .await
        .map_err(AdminServiceError::Database)?;

        transaction
            .commit()
            .await
            .map_err(AdminServiceError::Database)?;

        *live_exchange = staged_exchange;

        Ok(record)
    }

    pub async fn deposit(
        &self,
        reference_id: &str,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<DepositRecord, AdminServiceError> {
        let reference_id = reference_id.trim();

        if reference_id.is_empty() {
            return Err(AdminServiceError::InvalidDepositReference);
        }

        let database_user_id =
            i64::try_from(user_id.value()).map_err(|_| AdminServiceError::IdentifierOutOfRange)?;
        let mut live_exchange = self.exchange.lock().await;
        let mut staged_exchange = live_exchange.clone();
        let mut transaction = self.db.begin().await.map_err(AdminServiceError::Database)?;

        let user = users::find_by_id(transaction.as_mut(), database_user_id)
            .await
            .map_err(AdminServiceError::Database)?
            .ok_or(AdminServiceError::UnknownUser)?;
        if !user.enabled {
            return Err(AdminServiceError::UserDisabled);
        }

        let asset_record = assets::find_by_symbol(transaction.as_mut(), asset)
            .await
            .map_err(AdminServiceError::Database)?
            .ok_or(AdminServiceError::UnknownAsset)?;
        if !asset_record.enabled {
            return Err(AdminServiceError::AssetDisabled);
        }

        let operation_id = Uuid::new_v4();
        let amount_atomic = BigDecimal::from(amount.value());

        let deposit_insert = DepositInsert {
            id: operation_id,
            reference_id: reference_id.to_string(),
            user_id: database_user_id,
            asset_id: asset_record.id,
            amount_atomic: amount_atomic.clone(),
            status: "CREDITED".to_string(),
        };

        let deposit_record = match deposits::insert_if_absent(transaction.as_mut(), &deposit_insert)
            .await
            .map_err(AdminServiceError::Database)?
        {
            Some(record) => record,

            None => {
                let existing = deposits::find_by_reference(transaction.as_mut(), reference_id)
                    .await
                    .map_err(AdminServiceError::Database)?
                    .ok_or(AdminServiceError::UnexpectedEngineOutput)?;

                if existing.user_id != database_user_id
                    || existing.asset_id != asset_record.id
                    || existing.amount_atomic != amount_atomic
                {
                    return Err(AdminServiceError::DepositReferenceConflict);
                }

                transaction
                    .commit()
                    .await
                    .map_err(AdminServiceError::Database)?;

                return Ok(existing);
            }
        };

        let result = staged_exchange
            .deposit(user_id, asset, amount)
            .map_err(AdminServiceError::Exchange)?;

        let movement = result
            .balance_movements()
            .first()
            .ok_or(AdminServiceError::UnexpectedEngineOutput)?;

        let movement_insert = BalanceMovementInsert {
            operation_id: deposit_record.id,
            user_id: database_user_id,
            asset_id: asset_record.id,
            available_delta_atomic: delta_to_big_decimal(movement.available_delta()),
            locked_delta_atomic: delta_to_big_decimal(movement.locked_delta()),
            movement_type: movement_type(movement.reason()).to_string(),
            order_id: None,
            trade_id: None,
        };

        balance_movements::insert(transaction.as_mut(), &movement_insert)
            .await
            .map_err(AdminServiceError::Database)?;

        let snapshot = result
            .balance_snapshots()
            .first()
            .ok_or(AdminServiceError::UnexpectedEngineOutput)?;

        balances::upsert(
            transaction.as_mut(),
            database_user_id,
            asset_record.id,
            snapshot.available(),
            snapshot.locked(),
        )
        .await
        .map_err(AdminServiceError::Database)?;

        transaction
            .commit()
            .await
            .map_err(AdminServiceError::Database)?;

        *live_exchange = staged_exchange;

        Ok(deposit_record)
    }
}
