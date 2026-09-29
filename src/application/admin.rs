use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::{
    application::balance_mapping::{delta_to_big_decimal, movement_type},
    domain::{
        asset::{Asset, AssetSymbol},
        pair::TradingPair,
        primitives::{AssetAmount, Price, Quantity, UserId},
    },
    exchange::{BalanceOperationResult, Exchange, ExchangeError},
    persistence::postgres::{
        assets::{self, AssetRecord},
        balance_movements::{self, BalanceMovementInsert},
        balances,
        markets::{self, MarketRecord},
        users::{self, UserRecord},
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
    InvalidDisplayName,
    InvalidEmail,
    UnknownUser,
    UserDisabled,
    UnknownAsset,
    AssetDisabled,
    IdentifierOutOfRange,
    UnexpectedEngineOutput,
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

    pub async fn create_user(
        &self,
        display_name: &str,
        email: &str,
    ) -> Result<UserRecord, AdminServiceError> {
        let display_name = display_name.trim();

        if display_name.is_empty() {
            return Err(AdminServiceError::InvalidDisplayName);
        }

        let email = email.trim().to_ascii_lowercase();

        if email.is_empty() {
            return Err(AdminServiceError::InvalidEmail);
        }

        let mut transaction = self.db.begin().await.map_err(AdminServiceError::Database)?;

        let record = users::insert(transaction.as_mut(), display_name, &email)
            .await
            .map_err(AdminServiceError::Database)?;

        transaction
            .commit()
            .await
            .map_err(AdminServiceError::Database)?;

        Ok(record)
    }

    pub async fn deposit(
        &self,
        user_id: UserId,
        asset: &AssetSymbol,
        amount: AssetAmount,
    ) -> Result<BalanceOperationResult, AdminServiceError> {
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

        let result = staged_exchange
            .deposit(user_id, asset, amount)
            .map_err(AdminServiceError::Exchange)?;

        let movement = result
            .balance_movements()
            .first()
            .ok_or(AdminServiceError::UnexpectedEngineOutput)?;

        let operation_id = Uuid::new_v4();

        let movement_insert = BalanceMovementInsert {
            operation_id,
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

        Ok(result)
    }
}
