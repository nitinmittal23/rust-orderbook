use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    accounting::ledger::LedgerError,
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
