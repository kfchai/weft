"""Port of the 58 tests in ``bench/escape/weft/core.weft``.

Weft unit tests (``test "name" = <Bool expr>``, rule [W34]) become plain
pytest functions asserting the same expression. Weft property tests
(``test "name" (x: Int where ...) = <Bool expr>``, rule [W35]) become
hypothesis property tests whose strategies respect the parameter contracts.
"""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import strategies as st

from core import (
    Line,
    Order,
    OrderState,
    Pricing,
    StoreError,
    add_to_cart,
    best_seller,
    can_transition,
    cancel_and_restock,
    cart_count,
    cart_qty,
    cart_subtotal,
    catalog,
    clamp_total,
    display_name,
    find_product,
    format_cents,
    initial_stock,
    line_coupon_discount,
    make_coupon,
    new_order,
    pad2,
    place_order,
    price_cart,
    receipt,
    receipt_row,
    restock,
    ship_order,
    shipping_fee,
    stock_qty,
    take_stock,
    tier_discount,
    tier_percent,
    total_revenue,
    total_units,
    transition,
    unit_price,
)

# Weft's Int is a 64-bit machine integer; property strategies stay inside that
# range so generated values are ones the original test could also have seen.
INT_MAX = 2**63 - 1


# ------------------------------------------------------------
# Tests — 1. Money & catalog
# ------------------------------------------------------------


def test_catalog_has_six_products() -> None:
    """catalog has six products"""
    assert len(catalog) == 6


def test_find_product_hit() -> None:
    """find product hit"""
    p = find_product("MUG-001")
    assert p is not None
    assert p.name == "Stoneware Mug"


def test_find_product_miss() -> None:
    """find product miss"""
    assert find_product("NOPE") is None


def test_unit_price_ok() -> None:
    """unit price ok"""
    assert unit_price("HNY-001") == 950


def test_unit_price_unknown_sku() -> None:
    """unit price unknown sku"""
    with pytest.raises(StoreError) as excinfo:
        unit_price("NOPE")
    assert excinfo.value.message == "unknown sku: NOPE"


def test_format_cents_pads_sub_dime_remainders() -> None:
    """format cents pads sub-dime remainders"""
    assert format_cents(1205) == "$12.05"


def test_format_cents_zero() -> None:
    """format cents zero"""
    assert format_cents(0) == "$0.00"


@given(n=st.integers(min_value=0, max_value=INT_MAX))
def test_format_cents_always_has_a_dot(n: int) -> None:
    """format cents always has a dot"""
    assert "." in format_cents(n)


@given(n=st.integers(min_value=0, max_value=99))
def test_pad2_is_always_two_chars(n: int) -> None:
    """pad2 is always two chars"""
    assert len(pad2(n)) == 2


# ------------------------------------------------------------
# Tests — 2. Inventory
# ------------------------------------------------------------


def test_stock_qty_tracked() -> None:
    """stock qty tracked"""
    assert stock_qty(initial_stock, "POT-001") == 4


def test_stock_qty_unknown_is_zero() -> None:
    """stock qty unknown is zero"""
    assert stock_qty(initial_stock, "NOPE") == 0


def test_take_stock_reduces() -> None:
    """take stock reduces"""
    inv = take_stock(initial_stock, "POT-001", 3)
    assert stock_qty(inv, "POT-001") == 1


def test_take_stock_reports_insufficiency() -> None:
    """take stock reports insufficiency"""
    with pytest.raises(StoreError) as excinfo:
        take_stock(initial_stock, "POT-001", 5)
    assert "insufficient" in excinfo.value.message


def test_take_stock_conserves_other_skus() -> None:
    """take stock conserves other skus"""
    inv = take_stock(initial_stock, "TEA-001", 10)
    assert total_units(inv) == total_units(initial_stock) - 10


def test_restock_unknown_sku_errs() -> None:
    """restock unknown sku errs"""
    with pytest.raises(StoreError) as excinfo:
        restock(initial_stock, "NOPE", 5)
    assert excinfo.value.message == "cannot restock unknown sku: NOPE"


@given(q=st.integers(min_value=1, max_value=499))
def test_restock_then_take_round_trips(q: int) -> None:
    """restock then take round-trips"""
    up = restock(initial_stock, "MUG-001", q)
    down = take_stock(up, "MUG-001", q)
    assert down == initial_stock


# ------------------------------------------------------------
# Tests — 3. Cart
# ------------------------------------------------------------


def test_add_to_cart_appends_new_sku() -> None:
    """add to cart appends new sku"""
    assert add_to_cart((), "TEA-001", 2) == (Line(sku="TEA-001", qty=2),)


def test_add_to_cart_merges_duplicate_sku_into_one_line() -> None:
    """add to cart merges duplicate sku into one line"""
    assert len(add_to_cart(add_to_cart((), "TEA-001", 1), "TEA-001", 2)) == 1


@given(
    a=st.integers(min_value=1, max_value=999),
    b=st.integers(min_value=1, max_value=999),
)
def test_cart_merge_adds_quantities(a: int, b: int) -> None:
    """cart merge adds quantities"""
    assert cart_qty(add_to_cart(add_to_cart((), "X", a), "X", b), "X") == a + b


def test_cart_qty_absent_is_zero() -> None:
    """cart qty absent is zero"""
    assert cart_qty(add_to_cart((), "TEA-001", 2), "MUG-001") == 0


def test_cart_count_sums_units() -> None:
    """cart count sums units"""
    assert cart_count(add_to_cart(add_to_cart((), "TEA-001", 2), "HNY-001", 1)) == 3


def test_cart_subtotal_sums_line_costs() -> None:
    """cart subtotal sums line costs"""
    assert (
        cart_subtotal(add_to_cart(add_to_cart((), "TEA-001", 2), "HNY-001", 1)) == 3450
    )


def test_cart_subtotal_unknown_sku_errs() -> None:
    """cart subtotal unknown sku errs"""
    with pytest.raises(StoreError) as excinfo:
        cart_subtotal((Line(sku="NOPE", qty=1),))
    assert excinfo.value.message == "unknown sku: NOPE"


# ------------------------------------------------------------
# Tests — 4. Pricing
# ------------------------------------------------------------


def test_no_tier_at_or_under_10000() -> None:
    """no tier at or under 10000"""
    assert tier_percent(10000) == 0


def test_five_percent_tier_over_10000() -> None:
    """five percent tier over 10000"""
    assert tier_percent(10001) == 5


def test_ten_percent_tier_over_50000() -> None:
    """ten percent tier over 50000"""
    assert tier_percent(50001) == 10


def test_tier_discount_uses_integer_math() -> None:
    """tier discount uses integer math"""
    assert tier_discount(20000) == 1000


@given(s=st.integers(min_value=0, max_value=999999))
def test_tier_discount_stays_within_subtotal(s: int) -> None:
    """tier discount stays within subtotal"""
    d = tier_discount(s)
    assert d >= 0 and d <= s


def test_coupon_applies_to_its_sku() -> None:
    """coupon applies to its sku"""
    assert (
        line_coupon_discount(
            (make_coupon("TEA-001", 20),), Line(sku="TEA-001", qty=2)
        )
        == 500
    )


def test_coupon_skips_other_skus() -> None:
    """coupon skips other skus"""
    assert (
        line_coupon_discount(
            (make_coupon("TEA-001", 20),), Line(sku="HNY-001", qty=3)
        )
        == 0
    )


@given(
    s=st.integers(min_value=0, max_value=INT_MAX),
    d=st.integers(min_value=0, max_value=INT_MAX),
)
def test_final_total_never_negative(s: int, d: int) -> None:
    """final total never negative"""
    assert clamp_total(s, d) >= 0


def test_price_cart_combines_tier_and_coupon_discounts() -> None:
    """price cart combines tier and coupon discounts"""
    assert price_cart(
        add_to_cart((), "POT-001", 2), (make_coupon("POT-001", 10),)
    ) == Pricing(subtotal=12800, discount=1920, shipping=500, total=11380)


def test_shipping_fee_applied_below_threshold() -> None:
    """shipping fee applied below threshold"""
    assert shipping_fee(29999) == 500


def test_shipping_fee_waived_at_threshold() -> None:
    """shipping fee waived at threshold"""
    assert shipping_fee(30000) == 0


def test_shipping_fee_waived_above_threshold() -> None:
    """shipping fee waived above threshold"""
    assert shipping_fee(50000) == 0


def test_price_cart_charges_shipping_below_threshold() -> None:
    """price cart charges shipping below threshold"""
    assert price_cart(add_to_cart((), "TEA-001", 2), ()) == Pricing(
        subtotal=2500, discount=0, shipping=500, total=3000
    )


def test_price_cart_waives_shipping_above_threshold() -> None:
    """price cart waives shipping above threshold"""
    assert price_cart(add_to_cart((), "POT-001", 5), ()) == Pricing(
        subtotal=32000, discount=1600, shipping=0, total=30400
    )


def test_receipt_shows_shipping_fee_when_charged() -> None:
    """receipt shows shipping fee when charged"""
    r = receipt(new_order(21, add_to_cart((), "TEA-001", 2)), ())
    assert "Shipping: $5.00" in r and "total: $30.00" in r


def test_receipt_shows_zero_shipping_when_waived() -> None:
    """receipt shows zero shipping when waived"""
    r = receipt(new_order(22, add_to_cart((), "POT-001", 5)), ())
    assert "Shipping: $0.00" in r and "total: $304.00" in r


# ------------------------------------------------------------
# Tests — 5. Orders
# ------------------------------------------------------------


def test_draft_can_be_placed() -> None:
    """draft can be placed"""
    assert can_transition(OrderState.DRAFT, OrderState.PLACED)


def test_placed_can_be_cancelled() -> None:
    """placed can be cancelled"""
    assert can_transition(OrderState.PLACED, OrderState.CANCELLED)


def test_shipped_is_terminal() -> None:
    """shipped is terminal"""
    assert not can_transition(
        OrderState.SHIPPED, OrderState.CANCELLED
    ) and not can_transition(OrderState.SHIPPED, OrderState.DRAFT)


def test_illegal_transition_errs() -> None:
    """illegal transition errs"""
    with pytest.raises(StoreError) as excinfo:
        transition(new_order(1, ()), OrderState.SHIPPED)
    assert "illegal" in excinfo.value.message


def test_place_order_takes_stock_and_prices_total() -> None:
    """place order takes stock and prices total"""
    pl = place_order(new_order(2, add_to_cart((), "MUG-001", 2)), initial_stock, ())
    assert (
        stock_qty(pl.inv, "MUG-001") == 10
        and pl.order.state == OrderState.PLACED
        and pl.order.total_cents == 4100
    )


def test_place_order_fails_on_shortage() -> None:
    """place order fails on shortage"""
    with pytest.raises(StoreError) as excinfo:
        place_order(new_order(3, add_to_cart((), "POT-001", 99)), initial_stock, ())
    assert "insufficient" in excinfo.value.message


def test_empty_order_cannot_be_placed() -> None:
    """empty order cannot be placed"""
    with pytest.raises(StoreError) as excinfo:
        place_order(new_order(4, ()), initial_stock, ())
    assert "empty" in excinfo.value.message


def test_an_order_cannot_be_placed_twice() -> None:
    """an order cannot be placed twice"""
    pl = place_order(new_order(5, add_to_cart((), "HNY-001", 1)), initial_stock, ())
    with pytest.raises(StoreError) as excinfo:
        place_order(pl.order, pl.inv, ())
    assert "only draft" in excinfo.value.message


def test_ship_after_place_succeeds() -> None:
    """ship after place succeeds"""
    pl = place_order(new_order(6, add_to_cart((), "TEA-002", 1)), initial_stock, ())
    assert ship_order(pl.order) == Order(
        id=pl.order.id,
        lines=pl.order.lines,
        state=OrderState.SHIPPED,
        total_cents=pl.order.total_cents,
    )


def test_cancel_and_restock_restores_inventory() -> None:
    """cancel and restock restores inventory"""
    pl = place_order(new_order(7, add_to_cart((), "KTL-001", 2)), initial_stock, ())
    back = cancel_and_restock(pl.order, pl.inv)
    assert back.inv == initial_stock and back.order.state == OrderState.CANCELLED


# ------------------------------------------------------------
# Tests — 6. Reporting
# ------------------------------------------------------------


def test_revenue_counts_placed_and_shipped_only() -> None:
    """revenue counts placed and shipped only"""
    assert (
        total_revenue(
            (
                Order(id=1, lines=(), state=OrderState.PLACED, total_cents=100),
                Order(id=2, lines=(), state=OrderState.DRAFT, total_cents=999),
                Order(id=3, lines=(), state=OrderState.SHIPPED, total_cents=250),
                Order(id=4, lines=(), state=OrderState.CANCELLED, total_cents=400),
            )
        )
        == 350
    )


def test_best_seller_picks_the_highest_total_qty() -> None:
    """best seller picks the highest total qty"""
    assert (
        best_seller(
            (
                Order(
                    id=1,
                    lines=(Line(sku="A", qty=2),),
                    state=OrderState.PLACED,
                    total_cents=10,
                ),
                Order(
                    id=2,
                    lines=(Line(sku="B", qty=5),),
                    state=OrderState.SHIPPED,
                    total_cents=10,
                ),
            )
        )
        == "B"
    )


def test_best_seller_tie_goes_to_first_appearance() -> None:
    """best seller tie goes to first appearance"""
    assert (
        best_seller(
            (
                Order(
                    id=1,
                    lines=(Line(sku="A", qty=3),),
                    state=OrderState.PLACED,
                    total_cents=10,
                ),
                Order(
                    id=2,
                    lines=(Line(sku="B", qty=3),),
                    state=OrderState.PLACED,
                    total_cents=10,
                ),
            )
        )
        == "A"
    )


def test_best_seller_merges_across_orders() -> None:
    """best seller merges across orders"""
    assert (
        best_seller(
            (
                Order(
                    id=1,
                    lines=(Line(sku="A", qty=2),),
                    state=OrderState.PLACED,
                    total_cents=10,
                ),
                Order(
                    id=2,
                    lines=(Line(sku="B", qty=3),),
                    state=OrderState.PLACED,
                    total_cents=10,
                ),
                Order(
                    id=3,
                    lines=(Line(sku="A", qty=2),),
                    state=OrderState.SHIPPED,
                    total_cents=10,
                ),
            )
        )
        == "A"
    )


def test_cancelled_orders_sell_nothing() -> None:
    """cancelled orders sell nothing"""
    assert (
        best_seller(
            (
                Order(
                    id=1,
                    lines=(Line(sku="A", qty=9),),
                    state=OrderState.CANCELLED,
                    total_cents=10,
                ),
            )
        )
        is None
    )


def test_best_seller_of_no_orders_is_none() -> None:
    """best seller of no orders is None"""
    assert best_seller(()) is None


def test_receipt_names_products_and_totals() -> None:
    """receipt names products and totals"""
    r = receipt(new_order(8, add_to_cart((), "TEA-001", 2)), ())
    assert "Green Tea Tin" in r and "subtotal" in r and "$25.00" in r


def test_receipt_row_flags_unknown_skus() -> None:
    """receipt row flags unknown skus"""
    assert "unknown" in receipt_row(Line(sku="ZZZ", qty=1))


def test_display_name_falls_back_to_sku() -> None:
    """display name falls back to sku"""
    assert display_name("KTL-001") == "Gooseneck Kettle" and display_name("ZZZ") == "ZZZ"
