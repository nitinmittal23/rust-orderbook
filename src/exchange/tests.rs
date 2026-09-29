use super::*;

#[test]
fn registered_asset_can_be_retrieved() {
    let mut exchange = Exchange::new();
    let symbol = AssetSymbol::new("ETH").unwrap();
    let asset = Asset::new(symbol.clone(), 18).unwrap();
    exchange.register_asset(asset).unwrap();

    let stored_asset = exchange.asset(&symbol).unwrap();

    assert_eq!(stored_asset.symbol(), &symbol);
    assert_eq!(stored_asset.decimals(), 18);
}

#[test]
fn duplicate_asset_registration_is_rejected_without_replacement() {
    let mut exchange = Exchange::new();
    let symbol = AssetSymbol::new("ETH").unwrap();
    let original = Asset::new(symbol.clone(), 18).unwrap();
    let conflicting = Asset::new(symbol.clone(), 8).unwrap();
    exchange.register_asset(original).unwrap();

    assert_eq!(
        exchange.register_asset(conflicting),
        Err(ExchangeError::AssetAlreadyRegistered)
    );
    assert_eq!(exchange.asset(&symbol).unwrap().decimals(), 18);
}

#[test]
fn market_can_be_created_from_registered_assets() {
    let mut exchange = Exchange::new();
    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
    let _ = exchange.create_market(
        pair.clone(),
        Price::new(10_000).unwrap(),
        Quantity::new(100_000_000_000_000),
    );

    assert_eq!(exchange.market(&pair).unwrap().pair(), &pair);
}

#[test]
fn market_creation_rejects_unknown_base_asset() {
    let mut exchange = Exchange::new();
    let base_symbol = AssetSymbol::new("ETH").unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
    assert_eq!(
        exchange.create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        ),
        Err(ExchangeError::UnknownBaseAsset)
    )
}

#[test]
fn market_creation_rejects_unknown_quote_asset() {
    let mut exchange = Exchange::new();
    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();

    exchange.register_asset(base_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
    assert_eq!(
        exchange.create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        ),
        Err(ExchangeError::UnknownQuoteAsset)
    )
}

#[test]
fn duplicate_market_creation_is_rejected() {
    let mut exchange = Exchange::new();
    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();
    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    assert_eq!(
        exchange.create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        ),
        Err(ExchangeError::MarketAlreadyExists)
    );
}

#[test]
fn deposit_credits_registered_asset_balance() {
    let mut exchange = Exchange::new();
    let alice = UserId::new(1);
    let symbol = AssetSymbol::new("ETH").unwrap();
    let asset = Asset::new(symbol.clone(), 18).unwrap();
    exchange.register_asset(asset).unwrap();

    exchange
        .deposit(alice, &symbol, AssetAmount::new(1_000))
        .unwrap();

    let balance = exchange.ledger().balance(alice, &symbol);

    assert_eq!(balance.available(), AssetAmount::new(1_000));
    assert_eq!(balance.locked(), AssetAmount::new(0));
}

#[test]
fn deposit_rejects_unknown_asset() {
    let mut exchange = Exchange::new();
    let alice = UserId::new(1);
    let symbol = AssetSymbol::new("ETH").unwrap();

    assert_eq!(
        exchange.deposit(alice, &symbol, AssetAmount::new(1_000)),
        Err(ExchangeError::UnknownAsset)
    );
}

#[test]
fn deposit_propagates_ledger_error_without_changing_balance() {
    let mut exchange = Exchange::new();
    let alice = UserId::new(1);
    let usdc = AssetSymbol::new("ETH").unwrap();
    let asset = Asset::new(usdc.clone(), 18).unwrap();
    exchange.register_asset(asset).unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(u128::MAX))
        .unwrap();

    let balance_before = exchange.ledger().balance(alice, &usdc);

    assert_eq!(
        exchange.deposit(alice, &usdc, AssetAmount::new(1)),
        Err(ExchangeError::Ledger(LedgerError::Overflow))
    );

    let balance_after = exchange.ledger().balance(alice, &usdc);

    assert_eq!(balance_after, balance_before);
}

#[test]
fn required_lock_uses_quote_asset_for_buy() {
    let mut exchange = Exchange::new();

    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(3_000_000_000).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        )
        .unwrap();

    let required = exchange.required_lock(
        &pair,
        Side::Buy,
        Price::new(3_000_000_000).unwrap(),
        Quantity::new(2_000_000_000_000_000_000),
    );

    assert_eq!(
        required,
        Ok((pair.quote().clone(), AssetAmount::new(6_000_000_000)))
    );
}

#[test]
fn required_lock_uses_base_asset_for_sell() {
    let mut exchange = Exchange::new();

    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(3_000_000_000).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        )
        .unwrap();

    let required = exchange.required_lock(
        &pair,
        Side::Sell,
        Price::new(3_000_000_000).unwrap(),
        Quantity::new(2_000_000_000_000_000_000),
    );

    assert_eq!(
        required,
        Ok((
            pair.base().clone(),
            AssetAmount::new(2_000_000_000_000_000_000)
        ))
    );
}

#[test]
fn required_lock_rejects_invalid_market_increment() {
    let mut exchange = Exchange::new();

    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(3_000_000_000).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        )
        .unwrap();

    assert_eq!(
        exchange.required_lock(
            &pair,
            Side::Sell,
            Price::new(3_000_000_001).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        ),
        Err(ExchangeError::MarketOrder(
            MarketOrderError::PriceNotAligned
        ))
    );
}

#[test]
fn required_lock_rejects_unknown_market() {
    let mut exchange = Exchange::new();

    let base_symbol = AssetSymbol::new("ETH").unwrap();
    let base_asset = Asset::new(base_symbol.clone(), 18).unwrap();

    let quote_symbol = AssetSymbol::new("USDC").unwrap();
    let quote_asset = Asset::new(quote_symbol.clone(), 6).unwrap();

    exchange.register_asset(base_asset).unwrap();
    exchange.register_asset(quote_asset).unwrap();

    let pair = TradingPair::new(base_symbol, quote_symbol).unwrap();

    assert_eq!(
        exchange.required_lock(
            &pair,
            Side::Sell,
            Price::new(3_000_000_001).unwrap(),
            Quantity::new(2_000_000_000_000_000_000),
        ),
        Err(ExchangeError::UnknownMarket)
    );
}

#[test]
fn limit_buy_locks_quote_and_rests_on_market() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth, usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(10_000_000_000))
        .unwrap();
    let price = Price::new(3_000_000_000).unwrap();
    let quantity = Quantity::new(2_000_000_000_000_000_000);

    let result = exchange
        .place_limit_order(alice, &pair, Side::Buy, price, quantity)
        .unwrap();

    assert_eq!(result.order_id(), OrderId::new(1));
    assert!(result.trades().is_empty());
    assert_eq!(result.unfilled_quantity(), quantity);

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(6_000_000_000));
    assert_eq!(alice_usdc.available(), AssetAmount::new(4_000_000_000));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), Some(price));
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn crossing_limit_buy_settles_trade_and_refunds_price_improvement() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(4_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &eth, AssetAmount::new(1_000_000_000_000_000_000))
        .unwrap();

    let bob_price = Price::new(3_000_000_000).unwrap();
    let bob_quantity = Quantity::new(1_000_000_000_000_000_000);

    let _bob_result = exchange
        .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
        .unwrap();

    let alice_price = Price::new(3_100_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

    let result = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    assert_eq!(result.order_id(), OrderId::new(2));
    assert_eq!(result.trades().len(), 1);
    assert_eq!(result.unfilled_quantity(), Quantity::new(0));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(alice_usdc.available(), AssetAmount::new(1_000_000_000));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.available(),
        AssetAmount::new(1_000_000_000_000_000_000)
    );

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_usdc.available(), AssetAmount::new(3_000_000_000));
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));
    assert_eq!(bob_eth.available(), AssetAmount::new(0));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);

    let trade = &result.trades()[0];
    assert_eq!(trade.maker_order_id(), OrderId::new(1));
    assert_eq!(trade.taker_order_id(), OrderId::new(2));
    assert_eq!(trade.maker_user_id(), bob);
    assert_eq!(trade.taker_user_id(), alice);
    assert_eq!(trade.taker_side(), Side::Buy);
    assert_eq!(trade.price(), bob_price);
    assert_eq!(trade.quantity(), alice_quantity);
}

#[test]
fn crossing_limit_sell_settles_at_resting_buy_price() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(3_100_000_000))
        .unwrap();
    exchange
        .deposit(bob, &eth, AssetAmount::new(1_000_000_000_000_000_000))
        .unwrap();

    let alice_price = Price::new(3_100_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    let bob_price = Price::new(3_000_000_000).unwrap();
    let bob_quantity = Quantity::new(1_000_000_000_000_000_000);

    let result = exchange
        .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
        .unwrap();

    assert_eq!(result.order_id(), OrderId::new(2));
    assert_eq!(result.trades().len(), 1);
    assert_eq!(result.unfilled_quantity(), Quantity::new(0));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(alice_usdc.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.available(),
        AssetAmount::new(1_000_000_000_000_000_000)
    );

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_usdc.available(), AssetAmount::new(3_100_000_000));
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));
    assert_eq!(bob_eth.available(), AssetAmount::new(0));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);

    let trade = &result.trades()[0];
    assert_eq!(trade.maker_order_id(), OrderId::new(1));
    assert_eq!(trade.taker_order_id(), OrderId::new(2));
    assert_eq!(trade.maker_user_id(), alice);
    assert_eq!(trade.taker_user_id(), bob);
    assert_eq!(trade.taker_side(), Side::Sell);
    assert_eq!(trade.price(), alice_price);
    assert_eq!(trade.quantity(), alice_quantity);
}

#[test]
fn insufficient_funds_reject_order_without_changing_exchange_state() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);

    assert!(matches!(
        exchange.place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity),
        Err(ExchangeError::Ledger(LedgerError::InsufficientAvailable))
    ));
    let book = exchange.market(&pair).unwrap().order_book();
    assert_eq!(book.best_ask(), None);
    assert_eq!(book.best_bid(), None);

    let balance = exchange.ledger.balance(alice, &usdc);
    assert_eq!(balance.available(), AssetAmount::new(0));
    assert_eq!(balance.locked(), AssetAmount::new(0));

    exchange
        .deposit(alice, &usdc, AssetAmount::new(3_000_000_000))
        .unwrap();

    let successful = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    assert_eq!(successful.order_id(), OrderId::new(1));
}

#[test]
fn cancelling_partially_filled_buy_unlocks_remaining_quote() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(15_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &eth, AssetAmount::new(2_000_000_000_000_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    let bob_price = Price::new(3_000_000_000).unwrap();
    let bob_quantity = Quantity::new(2_000_000_000_000_000_000);

    let _bob_result = exchange
        .place_limit_order(bob, &pair, Side::Sell, bob_price, bob_quantity)
        .unwrap();

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(9_000_000_000));
    assert_eq!(alice_usdc.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.available(),
        AssetAmount::new(2_000_000_000_000_000_000)
    );

    let cancellation = exchange
        .cancel_order(alice, &pair, OrderId::new(1))
        .unwrap();
    let cancelled_order = cancellation.cancelled_order();

    assert_eq!(cancelled_order.id(), OrderId::new(1));
    assert_eq!(
        cancelled_order.original_quantity(),
        Quantity::new(5_000_000_000_000_000_000)
    );
    assert_eq!(
        cancelled_order.remaining_quantity(),
        Quantity::new(3_000_000_000_000_000_000)
    );
    let alice_usdc_after = exchange.ledger().balance(alice, &usdc);
    let alice_eth_after = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc_after.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_usdc_after.available(),
        AssetAmount::new(9_000_000_000)
    );
    assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth_after.available(),
        AssetAmount::new(2_000_000_000_000_000_000)
    );

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn another_user_cannot_cancel_order_or_unlock_its_funds() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(15_000_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    assert_eq!(
        exchange.cancel_order(bob, &pair, OrderId::new(1)),
        Err(ExchangeError::OrderNotOwnedByUser)
    );

    let alice_usdc = exchange.ledger().balance(alice, &usdc);

    assert_eq!(alice_usdc.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(15_000_000_000));

    let order_book = exchange.market(&pair).unwrap().order_book();
    assert_eq!(order_book.best_bid(), Some(alice_price));
    assert_eq!(order_book.best_ask(), None);

    let cancellation = exchange
        .cancel_order(alice, &pair, OrderId::new(1))
        .unwrap();
    let cancelled = cancellation.cancelled_order();

    assert_eq!(cancelled.id(), OrderId::new(1));
    assert_eq!(cancelled.user_id(), alice);

    let alice_usdc = exchange.ledger().balance(alice, &usdc);

    assert_eq!(alice_usdc.available(), AssetAmount::new(15_000_000_000));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
}

#[test]
fn cancelling_sell_unlocks_remaining_base() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .deposit(alice, &eth, AssetAmount::new(5_000_000_000_000_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(5_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
        .unwrap();

    let cancellation = exchange
        .cancel_order(alice, &pair, OrderId::new(1))
        .unwrap();
    let cancelled_order = cancellation.cancelled_order();

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);

    assert_eq!(alice_usdc.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.available(),
        AssetAmount::new(5_000_000_000_000_000_000)
    );
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    let order_book = exchange.market(&pair).unwrap().order_book();
    assert_eq!(order_book.best_ask(), None);

    assert_eq!(
        cancelled_order.remaining_quantity(),
        Quantity::new(5_000_000_000_000_000_000)
    );
}

#[test]
fn limit_buy_settles_multiple_makers_and_rests_remainder() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let carol = UserId::new(3);
    exchange
        .deposit(alice, &eth, AssetAmount::new(1_000_000_000_000_000_000))
        .unwrap();
    exchange
        .deposit(carol, &eth, AssetAmount::new(2_000_000_000_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &usdc, AssetAmount::new(12_080_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
        .unwrap();

    let carol_price = Price::new(3_010_000_000).unwrap();
    let carol_quantity = Quantity::new(2_000_000_000_000_000_000);

    let _carol_result = exchange
        .place_limit_order(carol, &pair, Side::Sell, carol_price, carol_quantity)
        .unwrap();

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_usdc.available(), AssetAmount::new(12_080_000_000));
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));
    assert_eq!(bob_eth.available(), AssetAmount::new(0));

    let bob_price = Price::new(3_020_000_000).unwrap();
    let bob_quantity = Quantity::new(4_000_000_000_000_000_000);

    let result = exchange
        .place_limit_order(bob, &pair, Side::Buy, bob_price, bob_quantity)
        .unwrap();

    let trades = result.trades();
    assert_eq!(trades.len(), 2);
    assert_eq!(
        result.unfilled_quantity(),
        Quantity::new(1_000_000_000_000_000_000)
    );

    assert_eq!(trades[0].maker_user_id(), alice);
    assert_eq!(trades[0].taker_user_id(), bob);
    assert_eq!(trades[0].price(), alice_price);
    assert_eq!(trades[0].quantity(), alice_quantity);

    assert_eq!(trades[1].maker_user_id(), carol);
    assert_eq!(trades[1].taker_user_id(), bob);
    assert_eq!(trades[1].price(), carol_price);
    assert_eq!(trades[1].quantity(), carol_quantity);

    let bob_usdc_after = exchange.ledger().balance(bob, &usdc);
    let bob_eth_after = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc_after.locked(), AssetAmount::new(3_020_000_000));
    assert_eq!(bob_usdc_after.available(), AssetAmount::new(40_000_000));
    assert_eq!(bob_eth_after.locked(), AssetAmount::new(0));
    assert_eq!(
        bob_eth_after.available(),
        AssetAmount::new(3_000_000_000_000_000_000)
    );

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(
        order_book.best_bid(),
        Some(Price::new(3_020_000_000).unwrap())
    );
    assert_eq!(order_book.best_ask(), None);

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);

    assert_eq!(alice_usdc.available(), AssetAmount::new(3_000_000_000));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    let carol_usdc = exchange.ledger().balance(carol, &usdc);
    let carol_eth = exchange.ledger().balance(carol, &eth);

    assert_eq!(carol_usdc.available(), AssetAmount::new(6_020_000_000));
    assert_eq!(carol_usdc.locked(), AssetAmount::new(0));
    assert_eq!(carol_eth.available(), AssetAmount::new(0));
    assert_eq!(carol_eth.locked(), AssetAmount::new(0));
}

#[test]
fn markets_keep_order_books_and_locked_funds_isolated() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();
    let pol = AssetSymbol::new("POL").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(pol.clone(), 18).unwrap())
        .unwrap();

    let eth_usdc_pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();
    let pol_usdc_pair = TradingPair::new(pol.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            eth_usdc_pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    exchange
        .create_market(
            pol_usdc_pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice = UserId::new(1);
    exchange
        .deposit(alice, &usdc, AssetAmount::new(4_000_000_000))
        .unwrap();

    let price_for_eth = Price::new(3_000_000_000).unwrap();
    let price_for_pol = Price::new(500_000).unwrap();
    let eth_quantity = Quantity::new(1_000_000_000_000_000_000);
    let pol_quantity = Quantity::new(100_000_000_000_000_000_000);

    let alice_eth_result = exchange
        .place_limit_order(
            alice,
            &eth_usdc_pair,
            Side::Buy,
            price_for_eth,
            eth_quantity,
        )
        .unwrap();
    assert_eq!(alice_eth_result.order_id(), OrderId::new(1));

    let alice_pol_result = exchange
        .place_limit_order(
            alice,
            &pol_usdc_pair,
            Side::Buy,
            price_for_pol,
            pol_quantity,
        )
        .unwrap();
    assert_eq!(alice_pol_result.order_id(), OrderId::new(2));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    let alice_pol = exchange.ledger().balance(alice, &pol);
    assert_eq!(alice_pol.locked(), AssetAmount::new(0));
    assert_eq!(alice_pol.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(3_050_000_000));
    assert_eq!(alice_usdc.available(), AssetAmount::new(950_000_000));

    assert_eq!(
        exchange
            .market(&eth_usdc_pair)
            .unwrap()
            .order_book()
            .best_bid(),
        Some(price_for_eth)
    );

    assert_eq!(
        exchange
            .market(&pol_usdc_pair)
            .unwrap()
            .order_book()
            .best_bid(),
        Some(price_for_pol)
    );

    let cancellation = exchange
        .cancel_order(alice, &eth_usdc_pair, OrderId::new(1))
        .unwrap();
    let cancelled_order = cancellation.cancelled_order();
    assert_eq!(cancelled_order.id(), OrderId::new(1));
    assert_eq!(cancelled_order.user_id(), alice);
    assert_eq!(cancelled_order.limit_price(), Some(price_for_eth));

    let alice_usdc_after = exchange.ledger().balance(alice, &usdc);
    let alice_eth_after = exchange.ledger().balance(alice, &eth);
    let alice_pol_after = exchange.ledger().balance(alice, &pol);
    assert_eq!(alice_pol_after.locked(), AssetAmount::new(0));
    assert_eq!(alice_pol_after.available(), AssetAmount::new(0));
    assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));
    assert_eq!(alice_eth_after.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc_after.locked(), AssetAmount::new(50_000_000));
    assert_eq!(
        alice_usdc_after.available(),
        AssetAmount::new(3_950_000_000)
    );

    let eth_usdc_order_book = exchange.market(&eth_usdc_pair).unwrap().order_book();
    let pol_usdc_order_book = exchange.market(&pol_usdc_pair).unwrap().order_book();

    assert_eq!(pol_usdc_order_book.best_bid(), Some(price_for_pol));
    assert_eq!(eth_usdc_order_book.best_bid(), None);
}

#[test]
fn market_buy_settles_liquidity_and_unlocks_unused_budget() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let carol = UserId::new(3);
    exchange
        .deposit(alice, &eth, AssetAmount::new(1_000_000_000_000_000_000))
        .unwrap();
    exchange
        .deposit(carol, &eth, AssetAmount::new(2_000_000_000_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &usdc, AssetAmount::new(12_080_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
        .unwrap();

    let carol_price = Price::new(3_010_000_000).unwrap();
    let carol_quantity = Quantity::new(2_000_000_000_000_000_000);

    let _carol_result = exchange
        .place_limit_order(carol, &pair, Side::Sell, carol_price, carol_quantity)
        .unwrap();

    let bob_quantity = Quantity::new(4_000_000_000_000_000_000);

    let result = exchange
        .place_market_order(
            bob,
            &pair,
            MarketOrderRequest::Buy {
                quantity: bob_quantity,
                max_quote_amount: AssetAmount::new(12_080_000_000),
            },
        )
        .unwrap();

    let trades = result.trades();
    assert_eq!(trades.len(), 2);
    assert_eq!(
        result.unfilled_quantity(),
        Quantity::new(1_000_000_000_000_000_000)
    );

    assert_eq!(trades[0].maker_user_id(), alice);
    assert_eq!(trades[0].taker_user_id(), bob);
    assert_eq!(trades[0].price(), alice_price);
    assert_eq!(trades[0].quantity(), alice_quantity);

    assert_eq!(trades[1].maker_user_id(), carol);
    assert_eq!(trades[1].taker_user_id(), bob);
    assert_eq!(trades[1].price(), carol_price);
    assert_eq!(trades[1].quantity(), carol_quantity);

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_usdc.available(), AssetAmount::new(3_060_000_000));
    assert_eq!(
        bob_eth.available(),
        AssetAmount::new(3_000_000_000_000_000_000)
    );

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc.available(), AssetAmount::new(3_000_000_000));

    let carol_usdc = exchange.ledger().balance(carol, &usdc);
    assert_eq!(carol_usdc.available(), AssetAmount::new(6_020_000_000));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn market_sell_unlocks_unfilled_base_and_does_not_rest() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    exchange
        .deposit(alice, &usdc, AssetAmount::new(9_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &eth, AssetAmount::new(5_000_000_000_000_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(3_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Buy, alice_price, alice_quantity)
        .unwrap();

    let bob_quantity = Quantity::new(5_000_000_000_000_000_000);

    let result = exchange
        .place_market_order(
            bob,
            &pair,
            MarketOrderRequest::Sell {
                quantity: bob_quantity,
            },
        )
        .unwrap();

    let trades = result.trades();
    assert_eq!(trades.len(), 1);
    assert_eq!(
        result.unfilled_quantity(),
        Quantity::new(2_000_000_000_000_000_000)
    );

    assert_eq!(trades[0].maker_user_id(), alice);
    assert_eq!(trades[0].taker_user_id(), bob);
    assert_eq!(trades[0].price(), alice_price);
    assert_eq!(trades[0].quantity(), alice_quantity);

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.available(), AssetAmount::new(9_000_000_000));
    assert_eq!(
        bob_eth.available(),
        AssetAmount::new(2_000_000_000_000_000_000)
    );
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.available(),
        AssetAmount::new(3_000_000_000_000_000_000)
    );

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn market_buy_over_budget_rejects_without_changing_exchange_state() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 18).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 6).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(
            pair.clone(),
            Price::new(10_000).unwrap(),
            Quantity::new(100_000_000_000_000),
        )
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    exchange
        .deposit(alice, &eth, AssetAmount::new(1_000_000_000_000_000_000))
        .unwrap();
    exchange
        .deposit(bob, &usdc, AssetAmount::new(2_999_000_000))
        .unwrap();

    let alice_price = Price::new(3_000_000_000).unwrap();
    let alice_quantity = Quantity::new(1_000_000_000_000_000_000);
    let _alice_result = exchange
        .place_limit_order(alice, &pair, Side::Sell, alice_price, alice_quantity)
        .unwrap();

    let bob_quantity = Quantity::new(1_000_000_000_000_000_000);

    assert!(matches!(
        exchange.place_market_order(
            bob,
            &pair,
            MarketOrderRequest::Buy {
                quantity: bob_quantity,
                max_quote_amount: AssetAmount::new(2_999_000_000),
            },
        ),
        Err(ExchangeError::MarketBuyBudgetExceeded),
    ));

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_usdc.available(), AssetAmount::new(2_999_000_000));
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_eth.available(), AssetAmount::new(0));
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(
        alice_eth.locked(),
        AssetAmount::new(1_000_000_000_000_000_000)
    );

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(
        order_book.best_ask(),
        Some(Price::new(3_000_000_000).unwrap())
    );
}

#[test]
fn stop_limit_sell_locks_base_without_entering_active_book() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(pair.clone(), Price::new(5).unwrap(), Quantity::new(20))
        .unwrap();

    exchange.deposit(alice, &eth, AssetAmount::new(40)).unwrap();

    let stop_price = Price::new(100).unwrap();
    let limit_price = Price::new(95).unwrap();
    let quantity = Quantity::new(40);

    let order_id = exchange
        .place_stop_limit_order(alice, &pair, Side::Sell, stop_price, limit_price, quantity)
        .unwrap()
        .order_id();

    assert_eq!(order_id, OrderId::new(1));

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.locked(), AssetAmount::new(40));
    assert_eq!(alice_eth.available(), AssetAmount::new(0));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn triggered_stop_limit_settles_prelocked_funds_and_rests_remainder() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(pair.clone(), Price::new(5).unwrap(), Quantity::new(20))
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let carol = UserId::new(3);

    exchange
        .deposit(bob, &usdc, AssetAmount::new(4_000))
        .unwrap();
    exchange.deposit(alice, &eth, AssetAmount::new(40)).unwrap();
    exchange.deposit(carol, &eth, AssetAmount::new(20)).unwrap();

    let bob_result = exchange
        .place_limit_order(
            bob,
            &pair,
            Side::Buy,
            Price::new(100).unwrap(),
            Quantity::new(40),
        )
        .unwrap();
    assert_eq!(bob_result.order_id(), OrderId::new(1));

    let stop_price = Price::new(100).unwrap();
    let limit_price = Price::new(95).unwrap();
    let quantity = Quantity::new(40);

    let order_id = exchange
        .place_stop_limit_order(alice, &pair, Side::Sell, stop_price, limit_price, quantity)
        .unwrap()
        .order_id();
    assert_eq!(order_id, OrderId::new(2));

    let carol_result = exchange
        .place_limit_order(
            carol,
            &pair,
            Side::Sell,
            Price::new(100).unwrap(),
            Quantity::new(20),
        )
        .unwrap();
    assert_eq!(carol_result.order_id(), OrderId::new(3));

    assert_eq!(carol_result.unfilled_quantity(), Quantity::new(0));
    let trades = carol_result.trades();

    assert_eq!(trades.len(), 2);

    assert_eq!(trades[0].maker_user_id(), bob);
    assert_eq!(trades[0].taker_user_id(), carol);
    assert_eq!(trades[0].price(), Price::new(100).unwrap());
    assert_eq!(trades[0].quantity(), Quantity::new(20));
    assert_eq!(trades[0].maker_order_id(), OrderId::new(1));
    assert_eq!(trades[0].taker_order_id(), OrderId::new(3));

    assert_eq!(trades[1].maker_user_id(), bob);
    assert_eq!(trades[1].taker_user_id(), alice);
    assert_eq!(trades[1].price(), Price::new(100).unwrap());
    assert_eq!(trades[1].quantity(), Quantity::new(20));
    assert_eq!(trades[1].maker_order_id(), OrderId::new(1));
    assert_eq!(trades[1].taker_order_id(), OrderId::new(2));

    let alice_eth = exchange.ledger().balance(alice, &eth);
    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_eth.locked(), AssetAmount::new(20));
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));
    assert_eq!(alice_usdc.available(), AssetAmount::new(2000));

    let bob_eth = exchange.ledger().balance(bob, &eth);
    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));
    assert_eq!(bob_eth.available(), AssetAmount::new(40));
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));
    assert_eq!(bob_usdc.available(), AssetAmount::new(0));

    let carol_eth = exchange.ledger().balance(carol, &eth);
    let carol_usdc = exchange.ledger().balance(carol, &usdc);
    assert_eq!(carol_eth.locked(), AssetAmount::new(0));
    assert_eq!(carol_eth.available(), AssetAmount::new(0));
    assert_eq!(carol_usdc.locked(), AssetAmount::new(0));
    assert_eq!(carol_usdc.available(), AssetAmount::new(2000));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), Some(Price::new(95).unwrap()));
}

#[test]
fn cancelling_pending_stop_unlocks_full_base_quantity() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();
    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();
    exchange
        .create_market(pair.clone(), Price::new(5).unwrap(), Quantity::new(20))
        .unwrap();

    let alice = UserId::new(1);
    exchange.deposit(alice, &eth, AssetAmount::new(40)).unwrap();

    let stop_price = Price::new(100).unwrap();
    let limit_price = Price::new(95).unwrap();

    exchange
        .place_stop_limit_order(
            alice,
            &pair,
            Side::Sell,
            stop_price,
            limit_price,
            Quantity::new(40),
        )
        .unwrap();

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(40));

    let cancellation = exchange
        .cancel_order(alice, &pair, OrderId::new(1))
        .unwrap();
    let cancelled = cancellation.cancelled_order();

    assert!(matches!(cancelled, CancelledOrder::PendingStop(_)));
    assert_eq!(cancelled.id(), OrderId::new(1));
    assert_eq!(cancelled.stop_price(), Some(stop_price));
    assert_eq!(cancelled.limit_price(), Some(limit_price));
    assert_eq!(cancelled.remaining_quantity(), Quantity::new(40));
    assert_eq!(cancelled.sequence(), None);

    let alice_eth_after = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth_after.available(), AssetAmount::new(40));
    assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));

    assert_eq!(
        exchange.cancel_order(alice, &pair, OrderId::new(1)),
        Err(ExchangeError::Cancel(CancelError::OrderNotFound))
    )
}

#[test]
fn another_user_cannot_cancel_pending_stop_or_unlock_funds() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();
    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();
    exchange
        .create_market(pair.clone(), Price::new(5).unwrap(), Quantity::new(20))
        .unwrap();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    exchange.deposit(alice, &eth, AssetAmount::new(40)).unwrap();

    let stop_price = Price::new(100).unwrap();
    let limit_price = Price::new(95).unwrap();

    exchange
        .place_stop_limit_order(
            alice,
            &pair,
            Side::Sell,
            stop_price,
            limit_price,
            Quantity::new(40),
        )
        .unwrap();

    assert_eq!(
        exchange.cancel_order(bob, &pair, OrderId::new(1)),
        Err(ExchangeError::OrderNotOwnedByUser)
    );

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(40));

    let cancellation = exchange
        .cancel_order(alice, &pair, OrderId::new(1))
        .unwrap();
    let cancelled = cancellation.cancelled_order();
    assert!(matches!(cancelled, CancelledOrder::PendingStop(_)));

    let alice_eth_after = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth_after.available(), AssetAmount::new(40));
    assert_eq!(alice_eth_after.locked(), AssetAmount::new(0));
}

#[test]
fn stop_limit_insufficient_funds_rolls_back_market_and_order_id() {
    let mut exchange = Exchange::new();

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();
    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();
    exchange
        .create_market(pair.clone(), Price::new(5).unwrap(), Quantity::new(20))
        .unwrap();

    let alice = UserId::new(1);
    exchange.deposit(alice, &eth, AssetAmount::new(20)).unwrap();

    let stop_price = Price::new(100).unwrap();
    let limit_price = Price::new(95).unwrap();

    assert_eq!(
        exchange.place_stop_limit_order(
            alice,
            &pair,
            Side::Sell,
            stop_price,
            limit_price,
            Quantity::new(40)
        ),
        Err(ExchangeError::Ledger(LedgerError::InsufficientAvailable))
    );

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(20));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    exchange.deposit(alice, &eth, AssetAmount::new(20)).unwrap();

    let order_id = exchange
        .place_stop_limit_order(
            alice,
            &pair,
            Side::Sell,
            stop_price,
            limit_price,
            Quantity::new(40),
        )
        .unwrap()
        .order_id();
    assert_eq!(order_id, OrderId::new(1));
}

#[test]
fn buy_stop_limit_locks_quote_and_remains_pending() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(pair.clone(), Price::new(1).unwrap(), Quantity::new(2))
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(300))
        .unwrap();

    let stop_price = Price::new(110).unwrap();
    let limit_price = Price::new(115).unwrap();
    let quantity = Quantity::new(2);

    let order_id = exchange
        .place_stop_limit_order(alice, &pair, Side::Buy, stop_price, limit_price, quantity)
        .unwrap()
        .order_id();

    assert_eq!(order_id, OrderId::new(1));

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc.locked(), AssetAmount::new(230));
    assert_eq!(alice_usdc.available(), AssetAmount::new(70));
    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    let order_book = exchange.market(&pair).unwrap().order_book();

    assert_eq!(order_book.best_bid(), None);
    assert_eq!(order_book.best_ask(), None);
}

#[test]
fn triggered_buy_stop_settles_trade_and_refunds_price_improvement() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let carol = UserId::new(3);
    let dave = UserId::new(4);

    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();
    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(pair.clone(), Price::new(1).unwrap(), Quantity::new(1))
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(230))
        .unwrap();

    exchange.deposit(bob, &eth, AssetAmount::new(1)).unwrap();

    exchange.deposit(carol, &eth, AssetAmount::new(2)).unwrap();

    exchange
        .deposit(dave, &usdc, AssetAmount::new(110))
        .unwrap();

    let alice_order_id = exchange
        .place_stop_limit_order(
            alice,
            &pair,
            Side::Buy,
            Price::new(110).unwrap(),
            Price::new(115).unwrap(),
            Quantity::new(2),
        )
        .unwrap()
        .order_id();

    assert_eq!(alice_order_id, OrderId::new(1));

    let bob_result = exchange
        .place_limit_order(
            bob,
            &pair,
            Side::Sell,
            Price::new(110).unwrap(),
            Quantity::new(1),
        )
        .unwrap();

    assert_eq!(bob_result.order_id(), OrderId::new(2));
    assert!(bob_result.trades().is_empty());

    let carol_result = exchange
        .place_limit_order(
            carol,
            &pair,
            Side::Sell,
            Price::new(112).unwrap(),
            Quantity::new(2),
        )
        .unwrap();

    assert_eq!(carol_result.order_id(), OrderId::new(3));
    assert!(carol_result.trades().is_empty());

    let dave_result = exchange
        .place_limit_order(
            dave,
            &pair,
            Side::Buy,
            Price::new(110).unwrap(),
            Quantity::new(1),
        )
        .unwrap();

    assert_eq!(dave_result.order_id(), OrderId::new(4));
    assert_eq!(dave_result.unfilled_quantity(), Quantity::new(0));

    let trades = dave_result.trades();
    assert_eq!(trades.len(), 2);

    assert_eq!(trades[0].maker_order_id(), OrderId::new(2));
    assert_eq!(trades[0].taker_order_id(), OrderId::new(4));
    assert_eq!(trades[0].maker_user_id(), bob);
    assert_eq!(trades[0].taker_user_id(), dave);
    assert_eq!(trades[0].taker_side(), Side::Buy);
    assert_eq!(trades[0].price(), Price::new(110).unwrap());
    assert_eq!(trades[0].quantity(), Quantity::new(1));
    assert_eq!(
        trades[0].taker_limit_price(),
        Some(Price::new(110).unwrap())
    );

    assert_eq!(trades[1].maker_order_id(), OrderId::new(3));
    assert_eq!(trades[1].taker_order_id(), OrderId::new(1));
    assert_eq!(trades[1].maker_user_id(), carol);
    assert_eq!(trades[1].taker_user_id(), alice);
    assert_eq!(trades[1].taker_side(), Side::Buy);
    assert_eq!(trades[1].price(), Price::new(112).unwrap());
    assert_eq!(trades[1].quantity(), Quantity::new(2));
    assert_eq!(
        trades[1].taker_limit_price(),
        Some(Price::new(115).unwrap())
    );

    let alice_usdc = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc.available(), AssetAmount::new(6));
    assert_eq!(alice_usdc.locked(), AssetAmount::new(0));

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(2));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    let bob_eth = exchange.ledger().balance(bob, &eth);
    assert_eq!(bob_eth.available(), AssetAmount::new(0));
    assert_eq!(bob_eth.locked(), AssetAmount::new(0));

    let bob_usdc = exchange.ledger().balance(bob, &usdc);
    assert_eq!(bob_usdc.available(), AssetAmount::new(110));
    assert_eq!(bob_usdc.locked(), AssetAmount::new(0));

    let carol_eth = exchange.ledger().balance(carol, &eth);
    assert_eq!(carol_eth.available(), AssetAmount::new(0));
    assert_eq!(carol_eth.locked(), AssetAmount::new(0));

    let carol_usdc = exchange.ledger().balance(carol, &usdc);
    assert_eq!(carol_usdc.available(), AssetAmount::new(224));
    assert_eq!(carol_usdc.locked(), AssetAmount::new(0));

    let dave_usdc = exchange.ledger().balance(dave, &usdc);
    assert_eq!(dave_usdc.available(), AssetAmount::new(0));
    assert_eq!(dave_usdc.locked(), AssetAmount::new(0));

    let dave_eth = exchange.ledger().balance(dave, &eth);
    assert_eq!(dave_eth.available(), AssetAmount::new(1));
    assert_eq!(dave_eth.locked(), AssetAmount::new(0));

    let market = exchange.market(&pair).unwrap();

    assert_eq!(market.last_trade_price(), Some(Price::new(112).unwrap()));
    assert_eq!(market.order_book().best_bid(), None);
    assert_eq!(market.order_book().best_ask(), None);
}

#[test]
fn cancelling_pending_buy_stop_unlocks_full_quote_amount() {
    let mut exchange = Exchange::new();

    let alice = UserId::new(1);
    let eth = AssetSymbol::new("ETH").unwrap();
    let usdc = AssetSymbol::new("USDC").unwrap();

    exchange
        .register_asset(Asset::new(eth.clone(), 0).unwrap())
        .unwrap();

    exchange
        .register_asset(Asset::new(usdc.clone(), 0).unwrap())
        .unwrap();

    let pair = TradingPair::new(eth.clone(), usdc.clone()).unwrap();

    exchange
        .create_market(pair.clone(), Price::new(1).unwrap(), Quantity::new(1))
        .unwrap();

    exchange
        .deposit(alice, &usdc, AssetAmount::new(230))
        .unwrap();

    let stop_price = Price::new(110).unwrap();
    let limit_price = Price::new(115).unwrap();
    let quantity = Quantity::new(2);

    let order_id = exchange
        .place_stop_limit_order(alice, &pair, Side::Buy, stop_price, limit_price, quantity)
        .unwrap()
        .order_id();

    assert_eq!(order_id, OrderId::new(1));

    let alice_usdc_before = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc_before.available(), AssetAmount::new(0));
    assert_eq!(alice_usdc_before.locked(), AssetAmount::new(230));

    let cancellation = exchange.cancel_order(alice, &pair, order_id).unwrap();
    let cancelled = cancellation.cancelled_order();

    assert!(matches!(&cancelled, CancelledOrder::PendingStop(_)));
    assert_eq!(cancelled.id(), order_id);
    assert_eq!(cancelled.user_id(), alice);
    assert_eq!(cancelled.side(), Side::Buy);
    assert_eq!(cancelled.stop_price(), Some(stop_price));
    assert_eq!(cancelled.limit_price(), Some(limit_price));
    assert_eq!(cancelled.original_quantity(), quantity);
    assert_eq!(cancelled.remaining_quantity(), quantity);
    assert_eq!(cancelled.sequence(), None);

    let alice_usdc_after = exchange.ledger().balance(alice, &usdc);
    assert_eq!(alice_usdc_after.available(), AssetAmount::new(230));
    assert_eq!(alice_usdc_after.locked(), AssetAmount::new(0));

    let alice_eth = exchange.ledger().balance(alice, &eth);
    assert_eq!(alice_eth.available(), AssetAmount::new(0));
    assert_eq!(alice_eth.locked(), AssetAmount::new(0));

    assert_eq!(
        exchange.cancel_order(alice, &pair, order_id),
        Err(ExchangeError::Cancel(CancelError::OrderNotFound))
    );
}
