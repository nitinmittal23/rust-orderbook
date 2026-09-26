#[derive(Debug, PartialEq, Eq)]
pub enum DecimalError {
    Empty,
    InvalidFormat,
    TooManyDecimalPlaces,
    Overflow,
}

pub fn parse_decimal(input: &str, decimals: u8) -> Result<u128, DecimalError> {
    if input.is_empty() {
        return Err(DecimalError::Empty);
    }

    let mut parts = input.split('.');

    let whole_part = parts.next().ok_or(DecimalError::InvalidFormat)?;
    let fractional_part = parts.next().unwrap_or("");

    if parts.next().is_some() {
        return Err(DecimalError::InvalidFormat);
    }

    if whole_part.is_empty() || (input.contains('.') && fractional_part.is_empty()) {
        return Err(DecimalError::InvalidFormat);
    }

    if !whole_part.bytes().all(|byte| byte.is_ascii_digit())
        || !fractional_part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(DecimalError::InvalidFormat);
    }

    if fractional_part.len() > decimals as usize {
        return Err(DecimalError::TooManyDecimalPlaces);
    }

    let scale = 10_u128
        .checked_pow(decimals as u32)
        .ok_or(DecimalError::Overflow)?;

    let whole_value = whole_part
        .parse::<u128>()
        .map_err(|_| DecimalError::Overflow)?;

    let whole_atomic = whole_value
        .checked_mul(scale)
        .ok_or(DecimalError::Overflow)?;

    let fractional_value = if fractional_part.is_empty() {
        0
    } else {
        fractional_part
            .parse::<u128>()
            .map_err(|_| DecimalError::Overflow)?
    };

    let missing_decimal_places = decimals as usize - fractional_part.len();

    let fractional_scale = 10_u128
        .checked_pow(missing_decimal_places as u32)
        .ok_or(DecimalError::Overflow)?;

    let fractional_atomic = fractional_value
        .checked_mul(fractional_scale)
        .ok_or(DecimalError::Overflow)?;

    whole_atomic
        .checked_add(fractional_atomic)
        .ok_or(DecimalError::Overflow)
}

pub fn format_decimal(value: u128, decimals: u8) -> Result<String, DecimalError> {
    let scale = 10_u128
        .checked_pow(decimals as u32)
        .ok_or(DecimalError::Overflow)?;

    let whole_part = value / scale;
    let remainder = value % scale;

    if remainder == 0 {
        return Ok(whole_part.to_string());
    }

    let mut fractional_part = format!("{remainder:0>width$}", width = decimals as usize,);

    while fractional_part.ends_with('0') {
        fractional_part.pop();
    }

    Ok(format!("{whole_part}.{fractional_part}"))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn decimal_value_is_converted_to_atomic_units() {
        assert_eq!(parse_decimal("3000.25", 6), Ok(3_000_250_000));
    }

    #[test]
    fn whole_value_is_scaled_to_atomic_units() {
        assert_eq!(parse_decimal("2", 18), Ok(2_000_000_000_000_000_000));
    }

    #[test]
    fn fractional_part_is_right_padded() {
        assert_eq!(parse_decimal("1.5", 6), Ok(1_500_000));
    }

    #[test]
    fn excess_decimal_places_are_rejected() {
        assert_eq!(
            parse_decimal("0.00001", 4),
            Err(DecimalError::TooManyDecimalPlaces)
        );
    }

    #[test]
    fn malformed_decimal_is_rejected() {
        assert_eq!(parse_decimal("12.3.4", 6), Err(DecimalError::InvalidFormat));
    }

    #[test]
    fn overflowing_decimal_is_rejected() {
        assert_eq!(
            parse_decimal("340282366920938463463374607431768211455", 1),
            Err(DecimalError::Overflow)
        );
    }

    #[test]
    fn atomic_decimal_value_is_formatted() {
        assert_eq!(format_decimal(3_000_250_000, 6), Ok("3000.25".to_string()),);
    }

    #[test]
    fn whole_atomic_value_omits_fraction() {
        assert_eq!(
            format_decimal(2_000_000_000_000_000_000, 18),
            Ok("2".to_string()),
        );
    }

    #[test]
    fn fractional_value_preserves_leading_zero() {
        assert_eq!(format_decimal(1_050_000, 6), Ok("1.05".to_string()),);
    }

    #[test]
    fn zero_is_formatted_without_fraction() {
        assert_eq!(format_decimal(0, 6), Ok("0".to_string()),);
    }
}
