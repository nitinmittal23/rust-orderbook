#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetSymbol(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSymbolError {
    Empty,
    InvalidCharacter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    UnsupportedDecimals,
}

#[derive(Clone)]
pub struct Asset {
    symbol: AssetSymbol,
    decimals: u8,
}

impl AssetSymbol {
    pub fn new(input: &str) -> Result<Self, AssetSymbolError> {
        let normalized = input.trim().to_ascii_uppercase();

        if normalized.is_empty() {
            return Err(AssetSymbolError::Empty);
        }

        if !normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
        {
            return Err(AssetSymbolError::InvalidCharacter);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Asset {
    pub fn new(symbol: AssetSymbol, decimals: u8) -> Result<Self, AssetError> {
        if decimals > 18 {
            return Err(AssetError::UnsupportedDecimals);
        }

        Ok(Self { symbol, decimals })
    }

    pub fn symbol(&self) -> &AssetSymbol {
        &self.symbol
    }

    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    pub fn scale(&self) -> u128 {
        10_u128.pow(self.decimals as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercase_symbol_is_accepted() {
        let symbol = AssetSymbol::new("ETH").unwrap();
        assert_eq!(symbol.as_str(), "ETH");
    }

    #[test]
    fn lowercase_symbol_is_normalized_to_uppercase() {
        let symbol = AssetSymbol::new("eth").unwrap();
        assert_eq!(symbol.as_str(), "ETH");
    }

    #[test]
    fn surrounding_whitespace_is_removed() {
        let symbol = AssetSymbol::new(" ETH ").unwrap();
        assert_eq!(symbol.as_str(), "ETH");
    }

    #[test]
    fn empty_symbol_is_rejected() {
        assert_eq!(AssetSymbol::new(""), Err(AssetSymbolError::Empty));
    }

    #[test]
    fn symbol_with_invalid_character_is_rejected() {
        assert_eq!(
            AssetSymbol::new("ETH/USDC"),
            Err(AssetSymbolError::InvalidCharacter)
        );
    }

    #[test]
    fn whitespace_only_symbol_is_rejected_as_empty() {
        assert_eq!(AssetSymbol::new("     "), Err(AssetSymbolError::Empty));
    }

    #[test]
    fn symbol_may_start_with_a_digit() {
        let symbol = AssetSymbol::new("1inch").unwrap();
        assert_eq!(symbol.as_str(), "1INCH");
    }

    #[test]
    fn asset_with_eighteen_decimals_is_accepted() {
        let symbol = AssetSymbol::new("ETH").unwrap();
        let asset = Asset::new(symbol, 18).unwrap();

        assert_eq!(asset.symbol().as_str(), "ETH");
        assert_eq!(asset.decimals(), 18);
        assert_eq!(asset.scale(), 10_u128.pow(18));
    }

    #[test]
    fn asset_scale_is_calculated_from_decimals() {
        let symbol = AssetSymbol::new("USDC").unwrap();
        let asset = Asset::new(symbol, 6).unwrap();
        assert_eq!(asset.scale(), 1_000_000);
    }

    #[test]
    fn asset_with_more_than_eighteen_decimals_is_rejected() {
        let symbol = AssetSymbol::new("TEST").unwrap();
        assert!(matches!(
            Asset::new(symbol, 19),
            Err(AssetError::UnsupportedDecimals)
        ));
    }
}
