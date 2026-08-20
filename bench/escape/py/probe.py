"""Differential probe for the Python arm.

Prints a transcript that must be line-for-line identical to the one produced
by the Weft arm (weft/probe.weft, built from core.weft + weft/probe_main.weft).
The harness compares the two transcripts to decide whether a mutation changed
observable behaviour at all; anything this probe fails to exercise would be
misreported as a semantically equivalent no-op.

Every printed line is the observable result of a call. Calls that can fail
print their failure as a value ("err:<message>") so a mutation late in the
module is never hidden by an early halt.
"""

from __future__ import annotations

from typing import Callable

from core import (
    Coupon,
    Line,
    Order,
    OrderState,
    Placement,
    Pricing,
    Product,
    Stock,
    StoreError,
    add_to_cart,
    best_seller,
    can_transition,
    cancel_and_restock,
    cancel_order,
    cart_count,
    cart_qty,
    cart_subtotal,
    catalog,
    clamp_total,
    coupon_discount,
    coupon_for,
    display_name,
    find_product,
    find_stock,
    format_cents,
    initial_stock,
    line_cost,
    line_coupon_discount,
    make_coupon,
    max_line,
    money_row,
    new_order,
    order_revenue,
    pad2,
    place_order,
    price_cart,
    product_or_err,
    receipt,
    receipt_row,
    restock,
    restock_lines,
    revenue_lines,
    ship_order,
    shipping_fee,
    sku_totals,
    state_name,
    stock_qty,
    take_lines,
    take_stock,
    tier_discount,
    tier_percent,
    total_revenue,
    total_units,
    transition,
    unit_price,
)


def pb_row(label: str, value: str) -> str:
    return label + " = " + value


def pb_bool(b: bool) -> str:
    return "true" if b else "false"


def pb_product(p: Product) -> str:
    return p.sku + "/" + p.name + "/" + str(p.unit_cents)


def pb_opt_product(o: Product | None) -> str:
    return "none" if o is None else "some:" + pb_product(o)


def pb_res_product(f: Callable[[], Product]) -> str:
    try:
        return "ok:" + pb_product(f())
    except StoreError as e:
        return "err:" + e.message


def pb_res_int(f: Callable[[], int]) -> str:
    try:
        return "ok:" + str(f())
    except StoreError as e:
        return "err:" + e.message


def pb_stock(s: Stock) -> str:
    return s.sku + ":" + str(s.qty)


def pb_opt_stock(o: Stock | None) -> str:
    return "none" if o is None else "some:" + pb_stock(o)


def pb_inv(inv: tuple[Stock, ...]) -> str:
    return "[" + " ".join(pb_stock(s) for s in inv) + "]"


def pb_res_inv(f: Callable[[], tuple[Stock, ...]]) -> str:
    try:
        return "ok:" + pb_inv(f())
    except StoreError as e:
        return "err:" + e.message


def pb_units(f: Callable[[], tuple[Stock, ...]]) -> str:
    try:
        return "ok:" + str(total_units(f()))
    except StoreError as e:
        return "err:" + e.message


def pb_line(l: Line) -> str:
    return l.sku + "x" + str(l.qty)


def pb_opt_line(o: Line | None) -> str:
    return "none" if o is None else "some:" + pb_line(o)


def pb_cart(cart: tuple[Line, ...]) -> str:
    return "[" + " ".join(pb_line(l) for l in cart) + "]"


def pb_coupon(c: Coupon) -> str:
    return c.sku + "@" + str(c.percent)


def pb_opt_coupon(o: Coupon | None) -> str:
    return "none" if o is None else "some:" + pb_coupon(o)


def pb_pricing(p: Pricing) -> str:
    return (
        "sub="
        + str(p.subtotal)
        + " disc="
        + str(p.discount)
        + " ship="
        + str(p.shipping)
        + " total="
        + str(p.total)
    )


def pb_res_pricing(f: Callable[[], Pricing]) -> str:
    try:
        return "ok:" + pb_pricing(f())
    except StoreError as e:
        return "err:" + e.message


def pb_order(o: Order) -> str:
    return (
        "#"
        + str(o.id)
        + " "
        + state_name(o.state)
        + " "
        + pb_cart(o.lines)
        + " total="
        + str(o.total_cents)
    )


def pb_res_order(f: Callable[[], Order]) -> str:
    try:
        return "ok:" + pb_order(f())
    except StoreError as e:
        return "err:" + e.message


def pb_placement(pl: Placement) -> str:
    return pb_order(pl.order) + " inv=" + pb_inv(pl.inv)


def pb_res_placement(f: Callable[[], Placement]) -> str:
    try:
        return "ok:" + pb_placement(f())
    except StoreError as e:
        return "err:" + e.message


def pb_opt_text(o: str | None) -> str:
    return "none" if o is None else "some:" + o


# ---- probe data ----

pb_skus: tuple[str, ...] = (
    "TEA-001",
    "TEA-002",
    "MUG-001",
    "POT-001",
    "HNY-001",
    "KTL-001",
    "ZZZ",
    "tea-001",
    "-",
)

pb_states: tuple[OrderState, ...] = (
    OrderState.DRAFT,
    OrderState.PLACED,
    OrderState.SHIPPED,
    OrderState.CANCELLED,
)

pb_no_stock: tuple[Stock, ...] = ()

pb_no_lines: tuple[Line, ...] = ()

pb_no_coupons: tuple[Coupon, ...] = ()

pb_coupons: tuple[Coupon, ...] = (
    make_coupon("TEA-001", 15),
    make_coupon("POT-001", 90),
    make_coupon("TEA-001", 1),
    make_coupon("HNY-001", 50),
)

pb_cart_a: tuple[Line, ...] = add_to_cart(pb_no_lines, "TEA-001", 2)

pb_cart_b: tuple[Line, ...] = add_to_cart(add_to_cart(pb_cart_a, "POT-001", 1), "TEA-001", 3)

pb_cart_c: tuple[Line, ...] = add_to_cart(pb_cart_b, "HNY-001", 4)

pb_cart_bad: tuple[Line, ...] = add_to_cart(pb_cart_c, "ZZZ", 1)

pb_cart_10000: tuple[Line, ...] = add_to_cart(pb_no_lines, "TEA-001", 8)

pb_cart_11250: tuple[Line, ...] = add_to_cart(pb_no_lines, "TEA-001", 9)

pb_cart_50000: tuple[Line, ...] = add_to_cart(
    add_to_cart(pb_no_lines, "POT-001", 5), "MUG-001", 10
)

pb_cart_50950: tuple[Line, ...] = add_to_cart(pb_cart_50000, "HNY-001", 1)

pb_cart_31550: tuple[Line, ...] = add_to_cart(
    add_to_cart(add_to_cart(pb_no_lines, "KTL-001", 3), "TEA-001", 1), "MUG-001", 2
)

pb_cart_31650: tuple[Line, ...] = add_to_cart(
    add_to_cart(add_to_cart(pb_no_lines, "KTL-001", 3), "MUG-001", 2), "TEA-002", 1
)

pb_cart_huge: tuple[Line, ...] = add_to_cart(pb_no_lines, "POT-001", 20)

pb_take_cases: tuple[Line, ...] = (
    Line(sku="POT-001", qty=1),
    Line(sku="POT-001", qty=3),
    Line(sku="POT-001", qty=4),
    Line(sku="POT-001", qty=5),
    Line(sku="POT-001", qty=99),
    Line(sku="TEA-001", qty=1),
    Line(sku="TEA-001", qty=40),
    Line(sku="TEA-001", qty=41),
    Line(sku="KTL-001", qty=6),
    Line(sku="KTL-001", qty=7),
    Line(sku="MUG-001", qty=12),
    Line(sku="MUG-001", qty=13),
    Line(sku="ZZZ", qty=1),
)

pb_restock_cases: tuple[Line, ...] = (
    Line(sku="POT-001", qty=1),
    Line(sku="POT-001", qty=3),
    Line(sku="POT-001", qty=100),
    Line(sku="TEA-001", qty=1),
    Line(sku="KTL-001", qty=2),
    Line(sku="ZZZ", qty=1),
)

pb_cost_cases: tuple[Line, ...] = (
    Line(sku="TEA-001", qty=0),
    Line(sku="TEA-001", qty=1),
    Line(sku="TEA-001", qty=2),
    Line(sku="TEA-001", qty=3),
    Line(sku="KTL-001", qty=7),
    Line(sku="HNY-001", qty=11),
    Line(sku="POT-001", qty=1),
    Line(sku="ZZZ", qty=1),
)

pb_coupon_line_cases: tuple[Line, ...] = (
    Line(sku="TEA-001", qty=1),
    Line(sku="TEA-001", qty=2),
    Line(sku="TEA-001", qty=3),
    Line(sku="POT-001", qty=1),
    Line(sku="POT-001", qty=2),
    Line(sku="HNY-001", qty=1),
    Line(sku="HNY-001", qty=3),
    Line(sku="KTL-001", qty=1),
    Line(sku="ZZZ", qty=1),
)

pb_clamp_subs: tuple[int, ...] = (0, 100, 100, 100, 100, 100, 0, 999999, 50000)

pb_clamp_discs: tuple[int, ...] = (0, 0, 50, 100, 101, 1000, 500, 1, 50000)

pb_orders: tuple[Order, ...] = (
    Order(
        id=1,
        lines=(Line(sku="TEA-001", qty=2),),
        state=OrderState.PLACED,
        total_cents=100,
    ),
    Order(
        id=2,
        lines=(Line(sku="MUG-001", qty=9),),
        state=OrderState.DRAFT,
        total_cents=999,
    ),
    Order(
        id=3,
        lines=(Line(sku="TEA-001", qty=1), Line(sku="HNY-001", qty=3)),
        state=OrderState.SHIPPED,
        total_cents=250,
    ),
    Order(
        id=4,
        lines=(Line(sku="POT-001", qty=7),),
        state=OrderState.CANCELLED,
        total_cents=400,
    ),
)

pb_orders_placed: tuple[Order, ...] = (
    Order(id=5, lines=(Line(sku="A", qty=2),), state=OrderState.PLACED, total_cents=10),
)

pb_orders_cancelled: tuple[Order, ...] = (
    Order(
        id=6, lines=(Line(sku="A", qty=9),), state=OrderState.CANCELLED, total_cents=10
    ),
)

pb_orders_draft: tuple[Order, ...] = (
    Order(id=7, lines=(Line(sku="A", qty=9),), state=OrderState.DRAFT, total_cents=10),
)

pb_orders_tie: tuple[Order, ...] = (
    Order(id=8, lines=(Line(sku="A", qty=3),), state=OrderState.PLACED, total_cents=10),
    Order(id=9, lines=(Line(sku="B", qty=3),), state=OrderState.SHIPPED, total_cents=10),
)

pb_no_orders: tuple[Order, ...] = ()

# An inventory that tracks skus with none on hand: a depleted sku must still
# be found by find_stock, and must still be restockable.
pb_depleted: tuple[Stock, ...] = (
    Stock(sku="TEA-001", qty=0),
    Stock(sku="TEA-002", qty=5),
    Stock(sku="MUG-001", qty=0),
    Stock(sku="POT-001", qty=1),
)

pb_depleted_cases: tuple[Line, ...] = (
    Line(sku="TEA-001", qty=1),
    Line(sku="TEA-002", qty=5),
    Line(sku="MUG-001", qty=2),
    Line(sku="POT-001", qty=1),
    Line(sku="POT-001", qty=2),
    Line(sku="KTL-001", qty=1),
)


# ---- probe entry point ----


def main() -> int:
    print(pb_row("len catalog", str(len(catalog))))
    print("\n".join(pb_row("catalog", pb_product(p)) for p in catalog))
    print(
        "\n".join(
            pb_row("find_product " + s, pb_opt_product(find_product(s))) for s in pb_skus
        )
    )
    print(
        "\n".join(
            pb_row("product_or_err " + s, pb_res_product(lambda: product_or_err(s)))
            for s in pb_skus
        )
    )
    print(
        "\n".join(
            pb_row("unit_price " + s, pb_res_int(lambda: unit_price(s)))
            for s in pb_skus
        )
    )
    print(
        "\n".join(
            pb_row("pad2 " + str(n), pad2(n))
            for n in [0, 1, 2, 8, 9, 10, 11, 19, 20, 50, 89, 90, 98, 99]
        )
    )
    print(
        "\n".join(
            pb_row("format_cents " + str(n), format_cents(n))
            for n in [
                0, 1, 5, 9, 10, 50, 99, 100, 101, 105, 110, 150, 199, 200, 999, 1000,
                1005, 1050, 1205, 1250, 9999, 10000, 10001, 100000, 123456, 999999,
            ]
        )
    )
    print(pb_row("initial_stock", pb_inv(initial_stock)))
    print(pb_row("len initial_stock", str(len(initial_stock))))
    print("\n".join(pb_row("stock", pb_stock(s)) for s in initial_stock))
    print(pb_row("total_units initial_stock", str(total_units(initial_stock))))
    print(pb_row("total_units empty", str(total_units(pb_no_stock))))
    print(
        "\n".join(
            pb_row("find_stock " + s, pb_opt_stock(find_stock(initial_stock, s)))
            for s in pb_skus
        )
    )
    print(
        pb_row(
            "find_stock empty TEA-001", pb_opt_stock(find_stock(pb_no_stock, "TEA-001"))
        )
    )
    print(
        "\n".join(
            pb_row("stock_qty " + s, str(stock_qty(initial_stock, s))) for s in pb_skus
        )
    )
    print(
        pb_row("stock_qty empty TEA-001", str(stock_qty(pb_no_stock, "TEA-001")))
    )
    print(
        "\n".join(
            pb_row(
                "take_stock " + pb_line(c),
                pb_res_inv(lambda: take_stock(initial_stock, c.sku, c.qty)),
            )
            for c in pb_take_cases
        )
    )
    print(
        pb_row(
            "take_stock empty TEA-001 1",
            pb_res_inv(lambda: take_stock(pb_no_stock, "TEA-001", 1)),
        )
    )
    print(
        "\n".join(
            pb_row(
                "restock " + pb_line(c),
                pb_res_inv(lambda: restock(initial_stock, c.sku, c.qty)),
            )
            for c in pb_restock_cases
        )
    )
    print(
        pb_row(
            "restock empty TEA-001 1",
            pb_res_inv(lambda: restock(pb_no_stock, "TEA-001", 1)),
        )
    )
    print(
        pb_row(
            "total_units after take POT-001 4",
            pb_units(lambda: take_stock(initial_stock, "POT-001", 4)),
        )
    )
    print(
        pb_row(
            "total_units after restock POT-001 100",
            pb_units(lambda: restock(initial_stock, "POT-001", 100)),
        )
    )
    print(
        pb_row(
            "add_to_cart empty TEA-001 1",
            pb_cart(add_to_cart(pb_no_lines, "TEA-001", 1)),
        )
    )
    print(pb_row("cart_a", pb_cart(pb_cart_a)))
    print(pb_row("cart_b", pb_cart(pb_cart_b)))
    print(pb_row("cart_c", pb_cart(pb_cart_c)))
    print(pb_row("cart_bad", pb_cart(pb_cart_bad)))
    print(
        pb_row("add_to_cart merge head", pb_cart(add_to_cart(pb_cart_c, "TEA-001", 10)))
    )
    print(
        pb_row("add_to_cart merge mid", pb_cart(add_to_cart(pb_cart_c, "POT-001", 10)))
    )
    print(
        pb_row("add_to_cart merge tail", pb_cart(add_to_cart(pb_cart_c, "HNY-001", 10)))
    )
    print(pb_row("add_to_cart new sku", pb_cart(add_to_cart(pb_cart_c, "KTL-001", 1))))
    print(
        "\n".join(
            pb_row("cart_qty cart_c " + s, str(cart_qty(pb_cart_c, s)))
            for s in ["TEA-001", "POT-001", "HNY-001", "KTL-001", "ZZZ"]
        )
    )
    print(pb_row("cart_qty empty TEA-001", str(cart_qty(pb_no_lines, "TEA-001"))))
    print(pb_row("cart_qty cart_bad ZZZ", str(cart_qty(pb_cart_bad, "ZZZ"))))
    print(pb_row("cart_count empty", str(cart_count(pb_no_lines))))
    print(pb_row("cart_count cart_a", str(cart_count(pb_cart_a))))
    print(pb_row("cart_count cart_b", str(cart_count(pb_cart_b))))
    print(pb_row("cart_count cart_c", str(cart_count(pb_cart_c))))
    print(pb_row("cart_count cart_bad", str(cart_count(pb_cart_bad))))
    print(
        "\n".join(
            pb_row("line_cost " + pb_line(l), pb_res_int(lambda: line_cost(l)))
            for l in pb_cost_cases
        )
    )
    print(pb_row("cart_subtotal empty", pb_res_int(lambda: cart_subtotal(pb_no_lines))))
    print(pb_row("cart_subtotal cart_a", pb_res_int(lambda: cart_subtotal(pb_cart_a))))
    print(pb_row("cart_subtotal cart_b", pb_res_int(lambda: cart_subtotal(pb_cart_b))))
    print(pb_row("cart_subtotal cart_c", pb_res_int(lambda: cart_subtotal(pb_cart_c))))
    print(
        pb_row("cart_subtotal cart_bad", pb_res_int(lambda: cart_subtotal(pb_cart_bad)))
    )
    print(
        pb_row(
            "cart_subtotal two unknown",
            pb_res_int(
                lambda: cart_subtotal(
                    (Line(sku="ZZZ", qty=1), Line(sku="YYY", qty=1))
                )
            ),
        )
    )
    print(
        pb_row(
            "cart_subtotal unknown at tail",
            pb_res_int(
                lambda: cart_subtotal(
                    (Line(sku="TEA-001", qty=1), Line(sku="ZZZ", qty=1))
                )
            ),
        )
    )
    print(
        "\n".join(
            pb_row("make_coupon TEA-001 " + str(n), pb_coupon(make_coupon("TEA-001", n)))
            for n in [1, 2, 45, 89, 90]
        )
    )
    print(pb_row("pb_coupons", " ".join(pb_coupon(c) for c in pb_coupons)))
    print(
        "\n".join(
            pb_row("tier_percent " + str(n), str(tier_percent(n)))
            for n in [
                0, 1, 9999, 10000, 10001, 10002, 20000, 49999, 50000, 50001, 50002,
                100000, 500000, 999999,
            ]
        )
    )
    print(
        "\n".join(
            pb_row("tier_discount " + str(n), str(tier_discount(n)))
            for n in [
                0, 1, 19, 20, 9999, 10000, 10001, 10019, 10020, 20000, 49999, 50000,
                50001, 50019, 50020, 100000, 500000, 999999,
            ]
        )
    )
    print(
        "\n".join(
            pb_row("coupon_for pb_coupons " + s, pb_opt_coupon(coupon_for(pb_coupons, s)))
            for s in ["TEA-001", "POT-001", "HNY-001", "KTL-001", "ZZZ"]
        )
    )
    print(
        pb_row(
            "coupon_for empty TEA-001",
            pb_opt_coupon(coupon_for(pb_no_coupons, "TEA-001")),
        )
    )
    print(
        "\n".join(
            pb_row(
                "line_coupon_discount " + pb_line(l),
                pb_res_int(lambda: line_coupon_discount(pb_coupons, l)),
            )
            for l in pb_coupon_line_cases
        )
    )
    print(
        pb_row(
            "line_coupon_discount 1pct TEA-001x1",
            pb_res_int(
                lambda: line_coupon_discount(
                    (make_coupon("TEA-001", 1),), Line(sku="TEA-001", qty=1)
                )
            ),
        )
    )
    print(
        pb_row(
            "line_coupon_discount 90pct TEA-001x1",
            pb_res_int(
                lambda: line_coupon_discount(
                    (make_coupon("TEA-001", 90),), Line(sku="TEA-001", qty=1)
                )
            ),
        )
    )
    print(
        pb_row(
            "line_coupon_discount no coupons TEA-001x2",
            pb_res_int(
                lambda: line_coupon_discount(pb_no_coupons, Line(sku="TEA-001", qty=2))
            ),
        )
    )
    print(
        pb_row(
            "coupon_discount empty empty",
            pb_res_int(lambda: coupon_discount(pb_no_coupons, pb_no_lines)),
        )
    )
    print(
        pb_row(
            "coupon_discount empty cart_c",
            pb_res_int(lambda: coupon_discount(pb_no_coupons, pb_cart_c)),
        )
    )
    print(
        pb_row(
            "coupon_discount pb_coupons empty",
            pb_res_int(lambda: coupon_discount(pb_coupons, pb_no_lines)),
        )
    )
    print(
        pb_row(
            "coupon_discount pb_coupons cart_a",
            pb_res_int(lambda: coupon_discount(pb_coupons, pb_cart_a)),
        )
    )
    print(
        pb_row(
            "coupon_discount pb_coupons cart_c",
            pb_res_int(lambda: coupon_discount(pb_coupons, pb_cart_c)),
        )
    )
    print(
        pb_row(
            "coupon_discount pb_coupons cart_bad",
            pb_res_int(lambda: coupon_discount(pb_coupons, pb_cart_bad)),
        )
    )
    print(
        "\n".join(
            pb_row(
                "clamp_total " + str(sub) + " " + str(disc),
                str(clamp_total(sub, disc)),
            )
            for sub, disc in zip(pb_clamp_subs, pb_clamp_discs)
        )
    )
    print(
        "\n".join(
            pb_row("shipping_fee " + str(n), str(shipping_fee(n)))
            for n in [0, 1, 499, 500, 29998, 29999, 30000, 30001, 30500, 50000, 999999]
        )
    )
    print(
        pb_row(
            "price_cart empty none",
            pb_res_pricing(lambda: price_cart(pb_no_lines, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart cart_a none",
            pb_res_pricing(lambda: price_cart(pb_cart_a, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart cart_a coupons",
            pb_res_pricing(lambda: price_cart(pb_cart_a, pb_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart cart_c none",
            pb_res_pricing(lambda: price_cart(pb_cart_c, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart cart_c coupons",
            pb_res_pricing(lambda: price_cart(pb_cart_c, pb_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub10000 none",
            pb_res_pricing(lambda: price_cart(pb_cart_10000, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub11250 none",
            pb_res_pricing(lambda: price_cart(pb_cart_11250, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub50000 none",
            pb_res_pricing(lambda: price_cart(pb_cart_50000, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub50950 none",
            pb_res_pricing(lambda: price_cart(pb_cart_50950, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub31550 none",
            pb_res_pricing(lambda: price_cart(pb_cart_31550, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart sub31650 none",
            pb_res_pricing(lambda: price_cart(pb_cart_31650, pb_no_coupons)),
        )
    )
    print(
        pb_row(
            "price_cart huge 90pct",
            pb_res_pricing(
                lambda: price_cart(pb_cart_huge, (make_coupon("POT-001", 90),))
            ),
        )
    )
    print(
        pb_row(
            "price_cart huge 1pct",
            pb_res_pricing(
                lambda: price_cart(pb_cart_huge, (make_coupon("POT-001", 1),))
            ),
        )
    )
    print(
        pb_row(
            "price_cart cart_bad coupons",
            pb_res_pricing(lambda: price_cart(pb_cart_bad, pb_coupons)),
        )
    )
    print("\n".join(pb_row("state_name", state_name(s)) for s in pb_states))
    print(
        "\n".join(
            "\n".join(
                pb_row(
                    "can_transition " + state_name(a) + " " + state_name(b),
                    pb_bool(can_transition(a, b)),
                )
                for b in pb_states
            )
            for a in pb_states
        )
    )
    print(
        "\n".join(
            "\n".join(
                pb_row(
                    "transition " + state_name(a) + " " + state_name(b),
                    pb_res_order(
                        lambda: transition(
                            Order(id=9, lines=pb_cart_a, state=a, total_cents=7), b
                        )
                    ),
                )
                for b in pb_states
            )
            for a in pb_states
        )
    )
    print(pb_row("new_order 1 empty", pb_order(new_order(1, pb_no_lines))))
    print(pb_row("new_order 2 cart_a", pb_order(new_order(2, pb_cart_a))))
    print(pb_row("new_order 3 cart_c", pb_order(new_order(3, pb_cart_c))))
    print(pb_row("new_order 4 cart_bad", pb_order(new_order(4, pb_cart_bad))))
    print(
        pb_row(
            "take_lines empty lines",
            pb_res_inv(lambda: take_lines(initial_stock, pb_no_lines)),
        )
    )
    print(
        pb_row("take_lines cart_a", pb_res_inv(lambda: take_lines(initial_stock, pb_cart_a)))
    )
    print(
        pb_row("take_lines cart_c", pb_res_inv(lambda: take_lines(initial_stock, pb_cart_c)))
    )
    print(
        pb_row(
            "take_lines cart_bad", pb_res_inv(lambda: take_lines(initial_stock, pb_cart_bad))
        )
    )
    print(
        pb_row(
            "take_lines exact POT-001x4",
            pb_res_inv(
                lambda: take_lines(initial_stock, (Line(sku="POT-001", qty=4),))
            ),
        )
    )
    print(
        pb_row(
            "take_lines short POT-001x5",
            pb_res_inv(
                lambda: take_lines(initial_stock, (Line(sku="POT-001", qty=5),))
            ),
        )
    )
    print(
        pb_row(
            "take_lines short at second",
            pb_res_inv(
                lambda: take_lines(
                    initial_stock,
                    (Line(sku="TEA-001", qty=1), Line(sku="POT-001", qty=5)),
                )
            ),
        )
    )
    print(
        pb_row(
            "take_lines short at first",
            pb_res_inv(
                lambda: take_lines(
                    initial_stock,
                    (Line(sku="POT-001", qty=5), Line(sku="TEA-001", qty=1)),
                )
            ),
        )
    )
    print(
        pb_row(
            "take_lines twice same sku",
            pb_res_inv(
                lambda: take_lines(
                    initial_stock,
                    (Line(sku="POT-001", qty=2), Line(sku="POT-001", qty=3)),
                )
            ),
        )
    )
    print(
        pb_row(
            "take_lines empty inv", pb_res_inv(lambda: take_lines(pb_no_stock, pb_cart_a))
        )
    )
    print(
        pb_row(
            "restock_lines empty lines",
            pb_res_inv(lambda: restock_lines(initial_stock, pb_no_lines)),
        )
    )
    print(
        pb_row(
            "restock_lines cart_a",
            pb_res_inv(lambda: restock_lines(initial_stock, pb_cart_a)),
        )
    )
    print(
        pb_row(
            "restock_lines cart_c",
            pb_res_inv(lambda: restock_lines(initial_stock, pb_cart_c)),
        )
    )
    print(
        pb_row(
            "restock_lines cart_bad",
            pb_res_inv(lambda: restock_lines(initial_stock, pb_cart_bad)),
        )
    )
    print(
        pb_row(
            "restock_lines unknown at head",
            pb_res_inv(
                lambda: restock_lines(
                    initial_stock,
                    (Line(sku="ZZZ", qty=1), Line(sku="TEA-001", qty=1)),
                )
            ),
        )
    )
    print(
        pb_row(
            "restock_lines empty inv",
            pb_res_inv(lambda: restock_lines(pb_no_stock, pb_cart_a)),
        )
    )
    print(
        pb_row(
            "place_order empty lines",
            pb_res_placement(
                lambda: place_order(
                    new_order(100, pb_no_lines), initial_stock, pb_no_coupons
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order cart_a none",
            pb_res_placement(
                lambda: place_order(
                    new_order(101, pb_cart_a), initial_stock, pb_no_coupons
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order cart_a coupons",
            pb_res_placement(
                lambda: place_order(new_order(102, pb_cart_a), initial_stock, pb_coupons)
            ),
        )
    )
    print(
        pb_row(
            "place_order cart_c coupons",
            pb_res_placement(
                lambda: place_order(new_order(103, pb_cart_c), initial_stock, pb_coupons)
            ),
        )
    )
    print(
        pb_row(
            "place_order cart_bad",
            pb_res_placement(
                lambda: place_order(
                    new_order(104, pb_cart_bad), initial_stock, pb_no_coupons
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order short",
            pb_res_placement(
                lambda: place_order(
                    new_order(105, (Line(sku="POT-001", qty=5),)),
                    initial_stock,
                    pb_no_coupons,
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order exact",
            pb_res_placement(
                lambda: place_order(
                    new_order(106, (Line(sku="POT-001", qty=4),)),
                    initial_stock,
                    pb_no_coupons,
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order empty inv",
            pb_res_placement(
                lambda: place_order(
                    new_order(107, pb_cart_a), pb_no_stock, pb_no_coupons
                )
            ),
        )
    )
    print(
        "\n".join(
            pb_row(
                "place_order from " + state_name(s),
                pb_res_placement(
                    lambda: place_order(
                        Order(id=108, lines=pb_cart_a, state=s, total_cents=0),
                        initial_stock,
                        pb_no_coupons,
                    )
                ),
            )
            for s in pb_states
        )
    )
    print(
        "\n".join(
            pb_row(
                "ship_order " + state_name(s),
                pb_res_order(
                    lambda: ship_order(
                        Order(id=110, lines=pb_cart_a, state=s, total_cents=3)
                    )
                ),
            )
            for s in pb_states
        )
    )
    print(
        "\n".join(
            pb_row(
                "cancel_order " + state_name(s),
                pb_res_order(
                    lambda: cancel_order(
                        Order(id=111, lines=pb_cart_a, state=s, total_cents=3)
                    )
                ),
            )
            for s in pb_states
        )
    )
    print(
        "\n".join(
            pb_row(
                "cancel_and_restock " + state_name(s),
                pb_res_placement(
                    lambda: cancel_and_restock(
                        Order(id=112, lines=pb_cart_a, state=s, total_cents=3),
                        initial_stock,
                    )
                ),
            )
            for s in pb_states
        )
    )
    try:
        pl = place_order(new_order(113, pb_cart_c), initial_stock, pb_coupons)
        round_trip = pb_res_placement(
            lambda: cancel_and_restock(pl.order, pl.inv)
        )
    except StoreError as e:
        round_trip = "err:" + e.message
    print(pb_row("cancel_and_restock round trip", round_trip))
    print(
        pb_row(
            "cancel_and_restock unknown lines",
            pb_res_placement(
                lambda: cancel_and_restock(
                    Order(
                        id=114,
                        lines=pb_cart_bad,
                        state=OrderState.PLACED,
                        total_cents=3,
                    ),
                    initial_stock,
                )
            ),
        )
    )
    print(
        pb_row(
            "cancel_and_restock empty lines",
            pb_res_placement(
                lambda: cancel_and_restock(
                    Order(
                        id=115,
                        lines=pb_no_lines,
                        state=OrderState.PLACED,
                        total_cents=3,
                    ),
                    initial_stock,
                )
            ),
        )
    )
    print(
        "\n".join(
            pb_row(
                "order_revenue " + state_name(s) + " 100",
                str(
                    order_revenue(
                        Order(id=120, lines=pb_no_lines, state=s, total_cents=100)
                    )
                ),
            )
            for s in pb_states
        )
    )
    print(
        pb_row(
            "order_revenue placed 0",
            str(
                order_revenue(
                    Order(
                        id=121,
                        lines=pb_no_lines,
                        state=OrderState.PLACED,
                        total_cents=0,
                    )
                )
            ),
        )
    )
    print(
        pb_row(
            "order_revenue shipped 999999",
            str(
                order_revenue(
                    Order(
                        id=122,
                        lines=pb_no_lines,
                        state=OrderState.SHIPPED,
                        total_cents=999999,
                    )
                )
            ),
        )
    )
    print(
        "\n".join(
            pb_row("order_revenue order", str(order_revenue(o))) for o in pb_orders
        )
    )
    print(pb_row("total_revenue none", str(total_revenue(pb_no_orders))))
    print(pb_row("total_revenue pb_orders", str(total_revenue(pb_orders))))
    print(pb_row("total_revenue placed", str(total_revenue(pb_orders_placed))))
    print(pb_row("total_revenue cancelled", str(total_revenue(pb_orders_cancelled))))
    print(pb_row("total_revenue draft", str(total_revenue(pb_orders_draft))))
    print(pb_row("total_revenue tie", str(total_revenue(pb_orders_tie))))
    print(pb_row("revenue_lines none", pb_cart(revenue_lines(pb_no_orders))))
    print(pb_row("revenue_lines pb_orders", pb_cart(revenue_lines(pb_orders))))
    print(pb_row("revenue_lines placed", pb_cart(revenue_lines(pb_orders_placed))))
    print(
        pb_row("revenue_lines cancelled", pb_cart(revenue_lines(pb_orders_cancelled)))
    )
    print(pb_row("revenue_lines draft", pb_cart(revenue_lines(pb_orders_draft))))
    print(pb_row("revenue_lines tie", pb_cart(revenue_lines(pb_orders_tie))))
    print(pb_row("sku_totals none", pb_cart(sku_totals(pb_no_orders))))
    print(pb_row("sku_totals pb_orders", pb_cart(sku_totals(pb_orders))))
    print(pb_row("sku_totals placed", pb_cart(sku_totals(pb_orders_placed))))
    print(pb_row("sku_totals cancelled", pb_cart(sku_totals(pb_orders_cancelled))))
    print(pb_row("sku_totals tie", pb_cart(sku_totals(pb_orders_tie))))
    print(pb_row("max_line empty", pb_opt_line(max_line(pb_no_lines))))
    print(
        pb_row("max_line single", pb_opt_line(max_line((Line(sku="A", qty=1),))))
    )
    print(
        pb_row(
            "max_line ascending",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=1), Line(sku="B", qty=2), Line(sku="C", qty=3))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line descending",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=3), Line(sku="B", qty=2), Line(sku="C", qty=1))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line tie at head",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=3), Line(sku="B", qty=3), Line(sku="C", qty=2))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line tie at tail",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=1), Line(sku="B", qty=9), Line(sku="C", qty=9))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line all equal",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=4), Line(sku="B", qty=4), Line(sku="C", qty=4))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line peak in middle",
            pb_opt_line(
                max_line(
                    (Line(sku="A", qty=1), Line(sku="B", qty=9), Line(sku="C", qty=2))
                )
            ),
        )
    )
    print(
        pb_row(
            "max_line zeros",
            pb_opt_line(max_line((Line(sku="A", qty=0), Line(sku="B", qty=0)))),
        )
    )
    print(pb_row("best_seller none", pb_opt_text(best_seller(pb_no_orders))))
    print(pb_row("best_seller pb_orders", pb_opt_text(best_seller(pb_orders))))
    print(pb_row("best_seller placed", pb_opt_text(best_seller(pb_orders_placed))))
    print(
        pb_row("best_seller cancelled", pb_opt_text(best_seller(pb_orders_cancelled)))
    )
    print(pb_row("best_seller draft", pb_opt_text(best_seller(pb_orders_draft))))
    print(pb_row("best_seller tie", pb_opt_text(best_seller(pb_orders_tie))))
    print(
        "\n".join(pb_row("display_name " + s, display_name(s)) for s in pb_skus)
    )
    print(
        "\n".join(
            pb_row("receipt_row " + pb_line(l), receipt_row(l))
            for l in [
                Line(sku="TEA-001", qty=0),
                Line(sku="TEA-001", qty=1),
                Line(sku="TEA-001", qty=2),
                Line(sku="TEA-002", qty=3),
                Line(sku="MUG-001", qty=1),
                Line(sku="POT-001", qty=2),
                Line(sku="HNY-001", qty=7),
                Line(sku="KTL-001", qty=1),
                Line(sku="ZZZ", qty=1),
                Line(sku="ZZZ", qty=0),
            ]
        )
    )
    print(
        "\n".join(
            pb_row("money_row " + str(n), money_row("label", n))
            for n in [0, 1, 9, 10, 99, 100, 101, 12345]
        )
    )
    print(pb_row("money_row Shipping", money_row("Shipping", 500)))
    print(receipt(new_order(300, pb_cart_c), pb_no_coupons))
    print(receipt(new_order(301, pb_cart_c), pb_coupons))
    print(receipt(new_order(302, pb_no_lines), pb_no_coupons))
    print(receipt(new_order(303, pb_cart_bad), pb_coupons))
    print(
        receipt(
            Order(id=304, lines=pb_cart_a, state=OrderState.SHIPPED, total_cents=999),
            pb_no_coupons,
        )
    )
    print(
        receipt(
            Order(id=305, lines=pb_cart_a, state=OrderState.CANCELLED, total_cents=0),
            pb_coupons,
        )
    )
    print(
        receipt(
            Order(id=306, lines=pb_cart_31650, state=OrderState.PLACED, total_cents=0),
            pb_no_coupons,
        )
    )
    pb_demo_cart = add_to_cart(
        add_to_cart(add_to_cart(pb_no_lines, "TEA-001", 2), "POT-001", 1), "TEA-001", 1
    )
    pb_demo_coupons = (make_coupon("TEA-001", 15),)
    print(pb_row("demo cart", pb_cart(pb_demo_cart)))
    try:
        demo = place_order(new_order(1001, pb_demo_cart), initial_stock, pb_demo_coupons)
        found = best_seller((demo.order,))
        print(
            "\n".join(
                [
                    receipt(demo.order, pb_demo_coupons),
                    pb_row("demo units left", str(total_units(demo.inv))),
                    pb_row(
                        "demo best seller",
                        display_name(found if found is not None else "n/a"),
                    ),
                    pb_row("demo best seller raw", pb_opt_text(found)),
                ]
            )
        )
    except StoreError as e:
        print(pb_row("demo failed", e.message))
    print(pb_row("pb_depleted", pb_inv(pb_depleted)))
    print(pb_row("total_units pb_depleted", str(total_units(pb_depleted))))
    print(
        "\n".join(
            pb_row("find_stock depleted " + s, pb_opt_stock(find_stock(pb_depleted, s)))
            for s in pb_skus
        )
    )
    print(
        "\n".join(
            pb_row("stock_qty depleted " + s, str(stock_qty(pb_depleted, s)))
            for s in pb_skus
        )
    )
    print(
        "\n".join(
            pb_row(
                "take_stock depleted " + pb_line(c),
                pb_res_inv(lambda: take_stock(pb_depleted, c.sku, c.qty)),
            )
            for c in pb_depleted_cases
        )
    )
    print(
        "\n".join(
            pb_row(
                "restock depleted " + pb_line(c),
                pb_res_inv(lambda: restock(pb_depleted, c.sku, c.qty)),
            )
            for c in pb_depleted_cases
        )
    )
    print(
        pb_row(
            "take_lines depleted stocked sku",
            pb_res_inv(
                lambda: take_lines(pb_depleted, (Line(sku="POT-001", qty=1),))
            ),
        )
    )
    print(
        pb_row(
            "take_lines depleted zero sku",
            pb_res_inv(
                lambda: take_lines(pb_depleted, (Line(sku="TEA-001", qty=1),))
            ),
        )
    )
    print(
        pb_row(
            "restock_lines depleted",
            pb_res_inv(
                lambda: restock_lines(
                    pb_depleted,
                    (Line(sku="TEA-001", qty=1), Line(sku="POT-001", qty=2)),
                )
            ),
        )
    )
    print(
        pb_row(
            "place_order depleted",
            pb_res_placement(
                lambda: place_order(
                    new_order(400, (Line(sku="POT-001", qty=1),)),
                    pb_depleted,
                    pb_no_coupons,
                )
            ),
        )
    )
    print(
        pb_row(
            "cancel_and_restock depleted",
            pb_res_placement(
                lambda: cancel_and_restock(
                    Order(
                        id=401,
                        lines=(Line(sku="TEA-001", qty=2),),
                        state=OrderState.PLACED,
                        total_cents=5,
                    ),
                    pb_depleted,
                )
            ),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
