"""orderflow — a single-file order-management system.

A faithful Python port of ``bench/escape/weft/core.weft``.

Domains: money/catalog, inventory, cart, pricing, orders, reporting.
All money is int cents; all math is integer math.

Porting conventions (mirroring the Weft source one-for-one):

* Records (``type X = {...}``) become ``@dataclass(frozen=True)``.
* Record invariants (``type X = {...} where <bool>``, rule [W42]) become a
  ``__post_init__`` check raising :class:`ValueError`.
* Parameter contracts (``p: T where <bool>``, rule [W28]) become an explicit
  precondition check at the top of the function raising :class:`ValueError`.
* ``Option[T]`` becomes ``T | None``.
* ``Result[T, E]`` becomes "return the success value, raise
  :class:`StoreError` for the ``Err`` case"; the ``?`` operator [W26] then
  becomes ordinary exception propagation.
* Payload-free nominal variants become :class:`enum.Enum`.
* Weft's ``/`` and ``%`` truncate toward zero [W25], so they are spelled
  :func:`_int_div` / :func:`_int_mod` rather than Python's flooring ``//``
  and ``%``.
* Weft lists are immutable, so ``List[T]`` becomes ``tuple[T, ...]``.
"""

from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import assert_never

__all__ = [
    "StoreError",
    "Product",
    "catalog",
    "find_product",
    "product_or_err",
    "unit_price",
    "pad2",
    "format_cents",
    "Stock",
    "initial_stock",
    "find_stock",
    "stock_qty",
    "take_stock",
    "restock",
    "total_units",
    "Line",
    "add_to_cart",
    "cart_qty",
    "cart_count",
    "line_cost",
    "cart_subtotal",
    "Coupon",
    "make_coupon",
    "tier_percent",
    "tier_discount",
    "coupon_for",
    "line_coupon_discount",
    "coupon_discount",
    "clamp_total",
    "Pricing",
    "shipping_fee",
    "price_cart",
    "OrderState",
    "Order",
    "Placement",
    "state_name",
    "can_transition",
    "transition",
    "new_order",
    "take_lines",
    "restock_lines",
    "place_order",
    "ship_order",
    "cancel_order",
    "cancel_and_restock",
    "order_revenue",
    "total_revenue",
    "revenue_lines",
    "sku_totals",
    "max_line",
    "best_seller",
    "display_name",
    "receipt_row",
    "money_row",
    "receipt",
    "main",
]


class StoreError(Exception):
    """The ``Err`` case of a Weft ``Result[T, Text]``, carrying its message."""

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


def _int_div(a: int, b: int) -> int:
    """Integer division truncating toward zero, as Weft's ``/`` does [W25]."""
    quotient = abs(a) // abs(b)
    return quotient if (a < 0) == (b < 0) else -quotient


def _int_mod(a: int, b: int) -> int:
    """Integer remainder truncating toward zero, as Weft's ``%`` does [W25]."""
    return a - _int_div(a, b) * b


# ------------------------------------------------------------
# 1. Money & catalog
# ------------------------------------------------------------


@dataclass(frozen=True)
class Product:
    """A product in the catalog; unit_cents is the price of one unit in cents."""

    sku: str
    name: str
    unit_cents: int


catalog: tuple[Product, ...] = (
    Product(sku="TEA-001", name="Green Tea Tin", unit_cents=1250),
    Product(sku="TEA-002", name="Earl Grey Tin", unit_cents=1350),
    Product(sku="MUG-001", name="Stoneware Mug", unit_cents=1800),
    Product(sku="POT-001", name="Cast Iron Teapot", unit_cents=6400),
    Product(sku="HNY-001", name="Wildflower Honey", unit_cents=950),
    Product(sku="KTL-001", name="Gooseneck Kettle", unit_cents=8900),
)
"""The full store catalog: every product this store can sell."""


def find_product(sku: str) -> Product | None:
    """Find a product by sku with stdlib find; None when the sku is unknown."""
    for p in catalog:
        if p.sku == sku:
            return p
    return None


def product_or_err(sku: str) -> Product:
    """Find a product by sku, or an Err naming the missing sku."""
    found = find_product(sku)
    if found is None:
        raise StoreError("unknown sku: " + sku)
    return found


def unit_price(sku: str) -> int:
    """The unit price in cents for a sku, or an Err for unknown skus."""
    p = product_or_err(sku)
    return p.unit_cents


def pad2(n: int) -> str:
    """Left-pad a small non-negative Int to two digits, for cents formatting."""
    if not (n >= 0 and n <= 99):
        raise ValueError(
            f"[W28] contract violation in pad2: n >= 0 and n <= 99 (n = {n})"
        )
    if n < 10:
        return "0" + str(n)
    return str(n)


def format_cents(cents: int) -> str:
    """Render a non-negative cents amount as dollars, e.g. 1250 -> "$12.50"."""
    if not (cents >= 0):
        raise ValueError(
            f"[W28] contract violation in format_cents: cents >= 0 (cents = {cents})"
        )
    return "$" + str(_int_div(cents, 100)) + "." + pad2(_int_mod(cents, 100))


# ------------------------------------------------------------
# 2. Inventory
# ------------------------------------------------------------


@dataclass(frozen=True)
class Stock:
    """A stock level for one sku; the [W42] invariant makes negative stock unrepresentable."""

    sku: str
    qty: int

    def __post_init__(self) -> None:
        if not (self.qty >= 0):
            raise ValueError(
                "[W42] invariant violation for Stock: qty >= 0 "
                f"(sku = {self.sku!r}, qty = {self.qty})"
            )


initial_stock: tuple[Stock, ...] = (
    Stock(sku="TEA-001", qty=40),
    Stock(sku="TEA-002", qty=25),
    Stock(sku="MUG-001", qty=12),
    Stock(sku="POT-001", qty=4),
    Stock(sku="HNY-001", qty=30),
    Stock(sku="KTL-001", qty=6),
)
"""The opening inventory for the demo store."""


def find_stock(inv: tuple[Stock, ...], sku: str) -> Stock | None:
    """The stock record for a sku, if the inventory tracks it."""
    for s in inv:
        if s.sku == sku:
            return s
    return None


def stock_qty(inv: tuple[Stock, ...], sku: str) -> int:
    """The quantity on hand for a sku; unknown skus count as zero."""
    found = find_stock(inv, sku)
    if found is None:
        return 0
    return found.qty


def take_stock(inv: tuple[Stock, ...], sku: str, qty: int) -> tuple[Stock, ...]:
    """Remove qty units of a sku from inventory, or Err when stock is insufficient."""
    if not (qty > 0):
        raise ValueError(
            f"[W28] contract violation in take_stock: qty > 0 (qty = {qty})"
        )
    if stock_qty(inv, sku) < qty:
        raise StoreError(
            "insufficient stock for "
            + sku
            + ": have "
            + str(stock_qty(inv, sku))
            + ", want "
            + str(qty)
        )
    return tuple(
        Stock(sku=s.sku, qty=s.qty - qty) if s.sku == sku else s for s in inv
    )


def restock(inv: tuple[Stock, ...], sku: str, qty: int) -> tuple[Stock, ...]:
    """Add qty units of a tracked sku back to inventory, or Err for unknown skus."""
    if not (qty > 0):
        raise ValueError(f"[W28] contract violation in restock: qty > 0 (qty = {qty})")
    found = find_stock(inv, sku)
    if found is None:
        raise StoreError("cannot restock unknown sku: " + sku)
    return tuple(
        Stock(sku=s.sku, qty=s.qty + qty) if s.sku == sku else s for s in inv
    )


def total_units(inv: tuple[Stock, ...]) -> int:
    """Total units on hand across every sku in the inventory."""
    acc = 0
    for s in inv:
        acc = acc + s.qty
    return acc


# ------------------------------------------------------------
# 3. Cart
# ------------------------------------------------------------


@dataclass(frozen=True)
class Line:
    """One cart line: a sku and how many units of it the shopper wants."""

    sku: str
    qty: int


def add_to_cart(cart: tuple[Line, ...], sku: str, qty: int) -> tuple[Line, ...]:
    """Add qty units of a sku to a cart, merging into an existing line for that sku."""
    if not (qty > 0):
        raise ValueError(
            f"[W28] contract violation in add_to_cart: qty > 0 (qty = {qty})"
        )
    found: Line | None = None
    for line in cart:
        if line.sku == sku:
            found = line
            break
    if found is None:
        return cart + (Line(sku=sku, qty=qty),)
    return tuple(
        Line(sku=line.sku, qty=line.qty + qty) if line.sku == sku else line
        for line in cart
    )


def cart_qty(cart: tuple[Line, ...], sku: str) -> int:
    """The quantity of a sku currently in the cart; zero when absent."""
    for line in cart:
        if line.sku == sku:
            return line.qty
    return 0


def cart_count(cart: tuple[Line, ...]) -> int:
    """Total number of units across all cart lines."""
    acc = 0
    for line in cart:
        acc = acc + line.qty
    return acc


def line_cost(l: Line) -> int:
    """The cost of one cart line in cents, or Err when the sku is not in the catalog."""
    price = unit_price(l.sku)
    return price * l.qty


def cart_subtotal(cart: tuple[Line, ...]) -> int:
    """The cart subtotal in cents before any discount, or the first lookup Err."""
    if len(cart) == 0:
        return 0
    l, rest = cart[0], cart[1:]
    head = line_cost(l)
    tail = cart_subtotal(rest)
    return head + tail


# ------------------------------------------------------------
# 4. Pricing
# ------------------------------------------------------------


@dataclass(frozen=True)
class Coupon:
    """A per-sku coupon granting percent off that sku's line cost."""

    sku: str
    percent: int


def make_coupon(sku: str, percent: int) -> Coupon:
    """Construct a coupon, enforcing that percent stays within 1..90."""
    if not (percent >= 1 and percent <= 90):
        raise ValueError(
            "[W28] contract violation in make_coupon: "
            f"percent >= 1 and percent <= 90 (percent = {percent})"
        )
    return Coupon(sku=sku, percent=percent)


def tier_percent(subtotal_cents: int) -> int:
    """The tier discount percent a subtotal earns: 10% over 50000, 5% over 10000."""
    if not (subtotal_cents >= 0):
        raise ValueError(
            "[W28] contract violation in tier_percent: subtotal_cents >= 0 "
            f"(subtotal_cents = {subtotal_cents})"
        )
    if subtotal_cents > 50000:
        return 10
    elif subtotal_cents > 10000:
        return 5
    else:
        return 0


def tier_discount(subtotal_cents: int) -> int:
    """The tier discount in cents for a subtotal, using integer math."""
    if not (subtotal_cents >= 0):
        raise ValueError(
            "[W28] contract violation in tier_discount: subtotal_cents >= 0 "
            f"(subtotal_cents = {subtotal_cents})"
        )
    return _int_div(subtotal_cents * tier_percent(subtotal_cents), 100)


def coupon_for(coupons: tuple[Coupon, ...], sku: str) -> Coupon | None:
    """The first coupon that applies to a sku, if any."""
    for c in coupons:
        if c.sku == sku:
            return c
    return None


def line_coupon_discount(coupons: tuple[Coupon, ...], l: Line) -> int:
    """The coupon discount in cents for one cart line, or Err for unknown skus."""
    cost = line_cost(l)
    c = coupon_for(coupons, l.sku)
    if c is None:
        return 0
    return _int_div(cost * c.percent, 100)


def coupon_discount(coupons: tuple[Coupon, ...], cart: tuple[Line, ...]) -> int:
    """The total coupon discount in cents across the whole cart."""
    if len(cart) == 0:
        return 0
    l, rest = cart[0], cart[1:]
    head = line_coupon_discount(coupons, l)
    tail = coupon_discount(coupons, rest)
    return head + tail


def clamp_total(subtotal_cents: int, discount_cents: int) -> int:
    """Subtract a discount from a subtotal without ever going below zero."""
    if not (subtotal_cents >= 0):
        raise ValueError(
            "[W28] contract violation in clamp_total: subtotal_cents >= 0 "
            f"(subtotal_cents = {subtotal_cents})"
        )
    if not (discount_cents >= 0):
        raise ValueError(
            "[W28] contract violation in clamp_total: discount_cents >= 0 "
            f"(discount_cents = {discount_cents})"
        )
    return max(0, subtotal_cents - discount_cents)


@dataclass(frozen=True)
class Pricing:
    """A fully priced cart: subtotal, combined discount, shipping fee, final total."""

    subtotal: int
    discount: int
    shipping: int
    total: int


def shipping_fee(discounted_cents: int) -> int:
    """The flat shipping fee in cents, waived at or above 30000c post-discount."""
    if not (discounted_cents >= 0):
        raise ValueError(
            "[W28] contract violation in shipping_fee: discounted_cents >= 0 "
            f"(discounted_cents = {discounted_cents})"
        )
    if discounted_cents >= 30000:
        return 0
    else:
        return 500


def price_cart(cart: tuple[Line, ...], coupons: tuple[Coupon, ...]) -> Pricing:
    """Price a cart end to end: subtotal, tier + coupon discounts, shipping, total."""
    sub = cart_subtotal(cart)
    coup = coupon_discount(coupons, cart)
    disc = tier_discount(sub) + coup
    discounted = clamp_total(sub, disc)
    ship = shipping_fee(discounted)
    return Pricing(subtotal=sub, discount=disc, shipping=ship, total=discounted + ship)


# ------------------------------------------------------------
# 5. Orders
# ------------------------------------------------------------


class OrderState(enum.Enum):
    """The order lifecycle state machine."""

    DRAFT = enum.auto()
    PLACED = enum.auto()
    SHIPPED = enum.auto()
    CANCELLED = enum.auto()


@dataclass(frozen=True)
class Order:
    """An order: an id, its cart lines, its lifecycle state, and its priced total."""

    id: int
    lines: tuple[Line, ...]
    state: OrderState
    total_cents: int


@dataclass(frozen=True)
class Placement:
    """The result of placing (or cancelling) an order: the order plus updated inventory."""

    order: Order
    inv: tuple[Stock, ...]


def state_name(s: OrderState) -> str:
    """A human-readable name for an order state."""
    match s:
        case OrderState.DRAFT:
            return "draft"
        case OrderState.PLACED:
            return "placed"
        case OrderState.SHIPPED:
            return "shipped"
        case OrderState.CANCELLED:
            return "cancelled"
        case _ as unreachable:
            assert_never(unreachable)


def can_transition(from_: OrderState, to: OrderState) -> bool:
    """Whether the state machine allows moving from one state to another."""
    match from_:
        case OrderState.DRAFT:
            match to:
                case OrderState.PLACED:
                    return True
                case OrderState.CANCELLED:
                    return True
                case OrderState.DRAFT | OrderState.SHIPPED:
                    return False
                case _ as unreachable_to_draft:
                    assert_never(unreachable_to_draft)
        case OrderState.PLACED:
            match to:
                case OrderState.SHIPPED:
                    return True
                case OrderState.CANCELLED:
                    return True
                case OrderState.DRAFT | OrderState.PLACED:
                    return False
                case _ as unreachable_to_placed:
                    assert_never(unreachable_to_placed)
        case OrderState.SHIPPED:
            return False
        case OrderState.CANCELLED:
            return False
        case _ as unreachable:
            assert_never(unreachable)


def transition(o: Order, to: OrderState) -> Order:
    """Move an order to a new state, or Err naming the illegal transition."""
    if can_transition(o.state, to):
        return Order(id=o.id, lines=o.lines, state=to, total_cents=o.total_cents)
    raise StoreError(
        "illegal transition: " + state_name(o.state) + " -> " + state_name(to)
    )


def new_order(id: int, lines: tuple[Line, ...]) -> Order:
    """Create a fresh draft order for a cart, not yet priced or placed."""
    return Order(id=id, lines=lines, state=OrderState.DRAFT, total_cents=0)


def take_lines(inv: tuple[Stock, ...], lines: tuple[Line, ...]) -> tuple[Stock, ...]:
    """Take stock for every line of an order, or Err on the first shortage."""
    if len(lines) == 0:
        return inv
    l, rest = lines[0], lines[1:]
    next_inv = take_stock(inv, l.sku, l.qty)
    return take_lines(next_inv, rest)


def restock_lines(inv: tuple[Stock, ...], lines: tuple[Line, ...]) -> tuple[Stock, ...]:
    """Restock every line of an order, or Err on the first unknown sku."""
    if len(lines) == 0:
        return inv
    l, rest = lines[0], lines[1:]
    next_inv = restock(inv, l.sku, l.qty)
    return restock_lines(next_inv, rest)


def place_order(
    o: Order, inv: tuple[Stock, ...], coupons: tuple[Coupon, ...]
) -> Placement:
    """Place a draft order: price it, take stock for every line, move it to Placed."""
    match o.state:
        case OrderState.DRAFT:
            if len(o.lines) == 0:
                raise StoreError("cannot place an empty order")
            priced = price_cart(o.lines, coupons)
            taken = take_lines(inv, o.lines)
            placed = transition(
                Order(
                    id=o.id, lines=o.lines, state=o.state, total_cents=priced.total
                ),
                OrderState.PLACED,
            )
            return Placement(order=placed, inv=taken)
        case OrderState.PLACED | OrderState.SHIPPED | OrderState.CANCELLED:
            raise StoreError(
                "only draft orders can be placed, got " + state_name(o.state)
            )
        case _ as unreachable:
            assert_never(unreachable)


def ship_order(o: Order) -> Order:
    """Ship a placed order, or Err if the transition is illegal."""
    return transition(o, OrderState.SHIPPED)


def cancel_order(o: Order) -> Order:
    """Cancel a draft or placed order, or Err if already shipped or cancelled."""
    return transition(o, OrderState.CANCELLED)


def cancel_and_restock(o: Order, inv: tuple[Stock, ...]) -> Placement:
    """Cancel a placed order and return its stock, yielding order + restored inventory."""
    cancelled = cancel_order(o)
    restored = restock_lines(inv, o.lines)
    return Placement(order=cancelled, inv=restored)


# ------------------------------------------------------------
# 6. Reporting
# ------------------------------------------------------------


def order_revenue(o: Order) -> int:
    """Revenue an order contributes: its total when placed or shipped, else zero."""
    match o.state:
        case OrderState.PLACED:
            return o.total_cents
        case OrderState.SHIPPED:
            return o.total_cents
        case OrderState.DRAFT | OrderState.CANCELLED:
            return 0
        case _ as unreachable:
            assert_never(unreachable)


def total_revenue(orders: tuple[Order, ...]) -> int:
    """Total revenue in cents across a list of orders."""
    acc = 0
    for o in orders:
        acc = acc + order_revenue(o)
    return acc


def revenue_lines(orders: tuple[Order, ...]) -> tuple[Line, ...]:
    """Every revenue-bearing line across a list of orders, in order of appearance."""
    acc: tuple[Line, ...] = ()
    for o in orders:
        if order_revenue(o) > 0:
            acc = acc + o.lines
    return acc


def sku_totals(orders: tuple[Order, ...]) -> tuple[Line, ...]:
    """Units sold per sku, merged in first-appearance order via cart merging."""
    acc: tuple[Line, ...] = ()
    for l in revenue_lines(orders):
        acc = add_to_cart(acc, l.sku, l.qty)
    return acc


def max_line(lines: tuple[Line, ...]) -> Line | None:
    """The line with the highest qty; ties keep the line appearing earlier."""
    if len(lines) == 0:
        return None
    l, rest = lines[0], lines[1:]
    best = max_line(rest)
    if best is None:
        return l
    if best.qty > l.qty:
        return best
    return l


def best_seller(orders: tuple[Order, ...]) -> str | None:
    """The best-selling sku across orders; ties go to the first-appearing sku."""
    l = max_line(sku_totals(orders))
    if l is None:
        return None
    return l.sku


def display_name(sku: str) -> str:
    """The display name for a sku, falling back to the raw sku when unknown."""
    p = find_product(sku)
    if p is None:
        return sku
    return p.name


def receipt_row(l: Line) -> str:
    """One formatted receipt row: "name x qty @ unit = amount"."""
    p = find_product(l.sku)
    if p is None:
        return "  " + l.sku + " x " + str(l.qty) + " (unknown sku)"
    return (
        "  "
        + p.name
        + " x "
        + str(l.qty)
        + " @ "
        + format_cents(p.unit_cents)
        + " = "
        + format_cents(p.unit_cents * l.qty)
    )


def money_row(label: str, cents: int) -> str:
    """A labelled money row for the receipt footer."""
    if not (cents >= 0):
        raise ValueError(
            f"[W28] contract violation in money_row: cents >= 0 (cents = {cents})"
        )
    return "  " + label + ": " + format_cents(cents)


def receipt(o: Order, coupons: tuple[Coupon, ...]) -> str:
    """A full multi-line receipt Text for an order and its coupons."""
    try:
        p = price_cart(o.lines, coupons)
    except StoreError as e:
        return "receipt error: " + e.message
    return "\n".join(
        ["=== order #" + str(o.id) + " (" + state_name(o.state) + ") ==="]
        + [receipt_row(l) for l in o.lines]
        + [
            money_row("subtotal", p.subtotal),
            money_row("discount", p.discount),
            money_row("Shipping", p.shipping),
            money_row("total", p.total),
        ]
    )


# ------------------------------------------------------------
# Demo entry point
# ------------------------------------------------------------


def main() -> int:
    """Demo: build a cart, place the order, print its receipt and a stock summary."""
    cart = add_to_cart(
        add_to_cart(add_to_cart((), "TEA-001", 2), "POT-001", 1), "TEA-001", 1
    )
    coupons = (make_coupon("TEA-001", 15),)
    try:
        pl = place_order(new_order(1001, cart), initial_stock, coupons)
    except StoreError as e:
        print("demo failed: " + e.message)
        return 1
    print(receipt(pl.order, coupons))
    print("units left in stock: " + str(total_units(pl.inv)))
    found = best_seller((pl.order,))
    seller = found if found is not None else "n/a"
    print("best seller: " + display_name(seller))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
