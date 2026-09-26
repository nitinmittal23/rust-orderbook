use orderbook::domain::asset::{Asset, AssetSymbol};
use orderbook::domain::pair::TradingPair;
use orderbook::domain::primitives::{AssetAmount, Price, Quantity, UserId};
use orderbook::exchange::Exchange;

pub fn create_development_exchange() -> Exchange {
    let mut exchange = Exchange::new();

    let eth_symbol = AssetSymbol::new("ETH").unwrap();
    let usdc_symbol = AssetSymbol::new("USDC").unwrap();
    let pol_symbol = AssetSymbol::new("POL").unwrap();

    let eth_asset = Asset::new(eth_symbol.clone(), 18).unwrap();
    let usdc_asset = Asset::new(usdc_symbol.clone(), 6).unwrap();
    let pol_asset = Asset::new(pol_symbol.clone(), 18).unwrap();

    exchange.register_asset(eth_asset).unwrap();
    exchange.register_asset(usdc_asset).unwrap();
    exchange.register_asset(pol_asset).unwrap();

    let eth_usdc_pair = TradingPair::new(eth_symbol.clone(), usdc_symbol.clone()).unwrap();

    let pol_usdc_pair = TradingPair::new(pol_symbol.clone(), usdc_symbol.clone()).unwrap();

    exchange
        .create_market(
            eth_usdc_pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .expect("ETH/USDC market configuration must be valid");

    exchange
        .create_market(
            pol_usdc_pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .expect("POL/USDC market configuration must be valid");

    let alice = UserId::new(1);
    let bob = UserId::new(2);

    exchange
        .deposit(alice, &usdc_symbol, AssetAmount::new(10_000_000_000))
        .expect("Alice's development deposit must succeed");

    exchange
        .deposit(
            bob,
            &eth_symbol,
            AssetAmount::new(10_000_000_000_000_000_000),
        )
        .expect("Bob's development deposit must succeed");

    exchange
}
