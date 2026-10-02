use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    accounting::ledger::LedgerError,
    application::{
        admin::AdminServiceError, market_data::MarketDataServiceError,
        trading::TradingServiceError, user::UserServiceError,
    },
    exchange::ExchangeError,
    matching::{book::CancelError, market::MarketOrderError},
};

#[derive(Serialize)]
struct ErrorResponse {
    code: String,
    message: String,
}

pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    pub fn bad_request(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    pub fn not_found(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    pub fn internal(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    pub fn unauthorized(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    pub fn forbidden(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    pub fn conflict(code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            code: self.code,
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<ExchangeError> for ApiError {
    fn from(error: ExchangeError) -> Self {
        match error {
            ExchangeError::UnknownMarket => Self::not_found("MARKET_NOT_FOUND", "market not found"),

            ExchangeError::Ledger(LedgerError::InsufficientAvailable) => {
                Self::bad_request("INSUFFICIENT_FUNDS", "insufficient available balance")
            }

            ExchangeError::MarketOrder(MarketOrderError::PriceNotAligned) => Self::bad_request(
                "INVALID_PRICE_INCREMENT",
                "price does not follow the market tick size",
            ),

            ExchangeError::MarketOrder(MarketOrderError::QuantityNotAligned) => Self::bad_request(
                "INVALID_QUANTITY_INCREMENT",
                "quantity does not follow the market step size",
            ),

            ExchangeError::MarketOrder(_) => Self::bad_request("INVALID_ORDER", "order is invalid"),

            ExchangeError::Cancel(CancelError::OrderNotFound) => {
                Self::not_found("ORDER_NOT_FOUND", "order not found")
            }

            ExchangeError::OrderNotOwnedByUser => {
                Self::forbidden("ORDER_NOT_OWNED", "order does not belong to this user")
            }

            ExchangeError::ZeroMarketBuyBudget => Self::bad_request(
                "INVALID_MAX_QUOTE_AMOUNT",
                "max quote amount must be greater than zero",
            ),

            ExchangeError::MarketBuyBudgetExceeded => Self::bad_request(
                "MARKET_BUY_BUDGET_EXCEEDED",
                "market buy cost exceeds max quote amount",
            ),

            _ => Self::internal("EXCHANGE_ERROR", "exchange could not process the request"),
        }
    }
}

impl From<TradingServiceError> for ApiError {
    fn from(error: TradingServiceError) -> Self {
        match error {
            TradingServiceError::Exchange(error) => Self::from(error),

            TradingServiceError::UnknownUser => Self::not_found("USER_NOT_FOUND", "user not found"),

            TradingServiceError::UserDisabled => {
                Self::forbidden("USER_DISABLED", "user is disabled")
            }

            TradingServiceError::UnknownBaseAsset | TradingServiceError::UnknownQuoteAsset => {
                Self::not_found("ASSET_NOT_FOUND", "asset not found")
            }

            TradingServiceError::BaseAssetDisabled | TradingServiceError::QuoteAssetDisabled => {
                Self::bad_request("ASSET_DISABLED", "asset is disabled")
            }

            TradingServiceError::UnknownMarket => {
                Self::not_found("MARKET_NOT_FOUND", "market not found")
            }

            TradingServiceError::MarketDisabled => {
                Self::bad_request("MARKET_DISABLED", "market is disabled")
            }

            TradingServiceError::Database(_) => {
                Self::internal("DATABASE_ERROR", "database operation failed")
            }

            TradingServiceError::IdentifierOutOfRange
            | TradingServiceError::UnexpectedEngineOutput => {
                Self::internal("TRADING_SERVICE_ERROR", "order could not be processed")
            }
        }
    }
}

impl From<AdminServiceError> for ApiError {
    fn from(error: AdminServiceError) -> Self {
        match error {
            AdminServiceError::InvalidAssetName => {
                Self::bad_request("INVALID_ASSET_NAME", "asset name cannot be empty")
            }

            AdminServiceError::Exchange(ExchangeError::AssetAlreadyRegistered) => {
                Self::conflict("ASSET_ALREADY_EXISTS", "asset already exists")
            }

            AdminServiceError::Database(_) => {
                Self::internal("DATABASE_ERROR", "database operation failed")
            }

            AdminServiceError::UnknownBaseAsset => {
                Self::not_found("BASE_ASSET_NOT_FOUND", "base asset not found")
            }

            AdminServiceError::UnknownQuoteAsset => {
                Self::not_found("QUOTE_ASSET_NOT_FOUND", "quote asset not found")
            }

            AdminServiceError::BaseAssetDisabled => {
                Self::bad_request("BASE_ASSET_DISABLED", "base asset is disabled")
            }

            AdminServiceError::QuoteAssetDisabled => {
                Self::bad_request("QUOTE_ASSET_DISABLED", "quote asset is disabled")
            }

            AdminServiceError::Exchange(ExchangeError::MarketAlreadyExists) => {
                Self::conflict("MARKET_ALREADY_EXISTS", "market already exists")
            }

            AdminServiceError::Exchange(ExchangeError::MarketCreation(_)) => Self::bad_request(
                "INVALID_MARKET_CONFIGURATION",
                "price tick and quantity step are incompatible",
            ),

            AdminServiceError::Exchange(error) => Self::from(error),

            AdminServiceError::InvalidDepositReference => Self::bad_request(
                "INVALID_DEPOSIT_REFERENCE",
                "deposit reference cannot be empty",
            ),

            AdminServiceError::DepositReferenceConflict => Self::conflict(
                "DEPOSIT_REFERENCE_CONFLICT",
                "deposit reference was already used for different deposit details",
            ),

            AdminServiceError::UnknownUser => Self::not_found("USER_NOT_FOUND", "user not found"),

            AdminServiceError::UserDisabled => Self::forbidden("USER_DISABLED", "user is disabled"),

            AdminServiceError::UnknownAsset => {
                Self::not_found("ASSET_NOT_FOUND", "asset not found")
            }

            AdminServiceError::AssetDisabled => {
                Self::bad_request("ASSET_DISABLED", "asset is disabled")
            }

            AdminServiceError::IdentifierOutOfRange => {
                Self::bad_request("INVALID_IDENTIFIER", "identifier is out of range")
            }

            AdminServiceError::UnexpectedEngineOutput => Self::internal(
                "ADMIN_SERVICE_ERROR",
                "admin operation produced an unexpected result",
            ),
        }
    }
}

impl From<UserServiceError> for ApiError {
    fn from(error: UserServiceError) -> Self {
        match error {
            UserServiceError::InvalidDisplayName => {
                Self::bad_request("INVALID_DISPLAY_NAME", "invalid display name")
            }

            UserServiceError::InvalidEmail => Self::bad_request("INVALID_EMAIL", "invalid email"),

            UserServiceError::EmailAlreadyExists => Self::conflict(
                "EMAIL_ALREADY_EXISTS",
                "a user with this email already exists",
            ),

            UserServiceError::Database(_) => {
                Self::internal("DATABASE_ERROR", "database operation failed")
            }
        }
    }
}

impl From<MarketDataServiceError> for ApiError {
    fn from(error: MarketDataServiceError) -> Self {
        match error {
            MarketDataServiceError::UnknownMarket => {
                Self::not_found("MARKET_NOT_FOUND", "market not found")
            }
            MarketDataServiceError::InvalidStoredTrade => {
                Self::internal("INVALID_STORED_TRADE", "persisted trade data is invalid")
            }

            MarketDataServiceError::InvalidCandleLimit => Self::bad_request(
                "INVALID_CANDLE_LIMIT",
                "candle limit must be between 1 and 100",
            ),

            MarketDataServiceError::InvalidCandleRange => Self::internal(
                "INVALID_CANDLE_RANGE",
                "candle time range could not be calculated",
            ),

            MarketDataServiceError::CandleVolumeOverflow => Self::internal(
                "CANDLE_VOLUME_OVERFLOW",
                "candle volume exceeded the supported range",
            ),
            MarketDataServiceError::Database(_) => {
                Self::internal("DATABASE_ERROR", "database operation failed")
            }
        }
    }
}
