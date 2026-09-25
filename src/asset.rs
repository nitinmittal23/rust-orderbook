#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetSymbol(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSymbolError {
    Empty,
    InvalidCharacter,
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
}
