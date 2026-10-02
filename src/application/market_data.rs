use chrono::{DateTime, Utc};
use num_traits::ToPrimitive;
use sqlx::PgPool;
use std::collections::BTreeMap;

use crate::{
    domain::{
        pair::TradingPair,
        primitives::{Price, Quantity},
    },
    persistence::postgres::{
        markets::{self, MarketSummaryRecord},
        trades::{self, TradeRecord},
    },
};

use crate::application::candle_interval::CandleInterval;

#[derive(Clone)]
pub struct MarketDataService {
    db: PgPool,
}

#[derive(Debug)]
pub enum MarketDataServiceError {
    Database(sqlx::Error),
    UnknownMarket,
    InvalidStoredTrade,
    CandleVolumeOverflow,
    InvalidCandleLimit,
    InvalidCandleRange,
}

#[derive(Debug)]
pub struct CandlesResult {
    market: MarketSummaryRecord,
    candles: Vec<Candle>,
}

impl CandlesResult {
    pub fn into_parts(self) -> (MarketSummaryRecord, Vec<Candle>) {
        (self.market, self.candles)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candle {
    start_time: DateTime<Utc>,
    open: Price,
    high: Price,
    low: Price,
    close: Price,
    volume: Quantity,
}

impl Candle {
    fn new(start_time: DateTime<Utc>, price: Price, quantity: Quantity) -> Self {
        Self {
            start_time,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: quantity,
        }
    }

    pub fn start_time(&self) -> &DateTime<Utc> {
        &self.start_time
    }

    pub fn open(&self) -> Price {
        self.open
    }

    pub fn high(&self) -> Price {
        self.high
    }

    pub fn low(&self) -> Price {
        self.low
    }

    pub fn close(&self) -> Price {
        self.close
    }

    pub fn volume(&self) -> Quantity {
        self.volume
    }

    fn apply_trade(
        &mut self,
        price: Price,
        quantity: Quantity,
    ) -> Result<(), MarketDataServiceError> {
        if price > self.high {
            self.high = price;
        }

        if price < self.low {
            self.low = price;
        }

        self.close = price;

        let volume = self
            .volume
            .value()
            .checked_add(quantity.value())
            .ok_or(MarketDataServiceError::CandleVolumeOverflow)?;

        self.volume = Quantity::new(volume);

        Ok(())
    }
}

#[derive(Debug)]
pub struct RecentTradesResult {
    market: MarketSummaryRecord,
    trades: Vec<TradeRecord>,
}

impl RecentTradesResult {
    pub fn into_parts(self) -> (MarketSummaryRecord, Vec<TradeRecord>) {
        (self.market, self.trades)
    }
}

impl MarketDataService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn list_markets(&self) -> Result<Vec<MarketSummaryRecord>, MarketDataServiceError> {
        let mut connection = self
            .db
            .acquire()
            .await
            .map_err(MarketDataServiceError::Database)?;

        markets::list_summaries(connection.as_mut())
            .await
            .map_err(MarketDataServiceError::Database)
    }

    pub async fn recent_trades(
        &self,
        pair: &TradingPair,
        limit: u32,
    ) -> Result<RecentTradesResult, MarketDataServiceError> {
        let mut connection = self
            .db
            .acquire()
            .await
            .map_err(MarketDataServiceError::Database)?;

        let market =
            markets::find_summary_by_symbols(connection.as_mut(), pair.base(), pair.quote())
                .await
                .map_err(MarketDataServiceError::Database)?
                .ok_or(MarketDataServiceError::UnknownMarket)?;

        let trades = trades::list_recent(connection.as_mut(), market.id, i64::from(limit))
            .await
            .map_err(MarketDataServiceError::Database)?;

        Ok(RecentTradesResult { market, trades })
    }

    fn aggregate_candles(
        records: &[TradeRecord],
        interval: CandleInterval,
    ) -> Result<Vec<Candle>, MarketDataServiceError> {
        let mut candles: BTreeMap<i64, Candle> = BTreeMap::new();

        for record in records {
            let price_value = record
                .price_atomic
                .to_u128()
                .ok_or(MarketDataServiceError::InvalidStoredTrade)?;

            let quantity_value = record
                .quantity_atomic
                .to_u128()
                .ok_or(MarketDataServiceError::InvalidStoredTrade)?;

            let price =
                Price::new(price_value).map_err(|_| MarketDataServiceError::InvalidStoredTrade)?;

            if quantity_value == 0 {
                return Err(MarketDataServiceError::InvalidStoredTrade);
            }

            let quantity = Quantity::new(quantity_value);

            let bucket_timestamp = interval.bucket_start(record.created_at.timestamp());

            let start_time = DateTime::<Utc>::from_timestamp(bucket_timestamp, 0)
                .ok_or(MarketDataServiceError::InvalidStoredTrade)?;

            if let Some(candle) = candles.get_mut(&bucket_timestamp) {
                candle.apply_trade(price, quantity)?;
            } else {
                candles.insert(bucket_timestamp, Candle::new(start_time, price, quantity));
            }
        }

        Ok(candles.into_values().collect())
    }

    pub async fn candles(
        &self,
        pair: &TradingPair,
        interval: CandleInterval,
        limit: u32,
    ) -> Result<CandlesResult, MarketDataServiceError> {
        if !(1..=100).contains(&limit) {
            return Err(MarketDataServiceError::InvalidCandleLimit);
        }

        let mut connection = self
            .db
            .acquire()
            .await
            .map_err(MarketDataServiceError::Database)?;

        let market =
            markets::find_summary_by_symbols(connection.as_mut(), pair.base(), pair.quote())
                .await
                .map_err(MarketDataServiceError::Database)?
                .ok_or(MarketDataServiceError::UnknownMarket)?;

        let interval_seconds = interval.seconds();
        let current_bucket =
            interval.bucket_start(Utc::now().timestamp());
        
        let end_timestamp = current_bucket
            .checked_add(interval_seconds)
            .ok_or(MarketDataServiceError::InvalidCandleRange)?;

        let range_seconds = interval_seconds
            .checked_mul(i64::from(limit))
            .ok_or(MarketDataServiceError::InvalidCandleRange)?;

        let start_timestamp = end_timestamp
            .checked_sub(range_seconds)
            .ok_or(MarketDataServiceError::InvalidCandleRange)?;

        let start = DateTime::<Utc>::from_timestamp(start_timestamp, 0)
            .ok_or(MarketDataServiceError::InvalidCandleRange)?;

        let end = DateTime::<Utc>::from_timestamp(end_timestamp, 0)
            .ok_or(MarketDataServiceError::InvalidCandleRange)?;

        let records = trades::list_between(connection.as_mut(), market.id, start, end)
            .await
            .map_err(MarketDataServiceError::Database)?;

        let candles = Self::aggregate_candles(&records, interval)?;

        Ok(CandlesResult { market, candles })
    }
}
