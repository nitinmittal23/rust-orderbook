use crate::domain::asset::AssetSymbol;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradingPair {
    base: AssetSymbol,
    quote: AssetSymbol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradingPairError {
    SameAsset,
}

impl TradingPair {
    pub fn new(base: AssetSymbol, quote: AssetSymbol) -> Result<Self, TradingPairError> {
        if base == quote {
            return Err(TradingPairError::SameAsset);
        }
        Ok(Self { base, quote })
    }

    pub fn base(&self) -> &AssetSymbol {
        &self.base
    }

    pub fn quote(&self) -> &AssetSymbol {
        &self.quote
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_assets_form_a_trading_pair() {
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("USDC").unwrap();

        let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

        assert_eq!(pair.base(), &eth);
        assert_eq!(pair.quote(), &usdc);
    }

    #[test]
    fn same_asset_pair_is_rejected() {
        let eth = AssetSymbol::new("ETH").unwrap();
        let usdc = AssetSymbol::new("ETH").unwrap();

        assert_eq!(
            TradingPair::new(eth.clone(), usdc.clone()),
            Err(TradingPairError::SameAsset)
        );
    }
}
