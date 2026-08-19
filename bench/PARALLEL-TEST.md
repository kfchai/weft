# Parallel-editing test — do definitions, not files, decide conflicts?

**The claim** (from the founding design discussion): with definition-level
granularity, "parallel agents conflict only when they touch the *same
definition*, not the same file." Never tested until now.

**Setup.** One file: `store/orderflow.weft` (2,468 lines, 187 defs, 243
tests), baseline committed at e46e76d. Five agents run **concurrently**, each
seeing only `SPEC.md` and its own `weftc ctx` slice — never the file, never
each other's work, never each other's tasks.

- **T1–T4 target disjoint definitions** in four domains. If the claim holds,
  all four patches splice into the one file and the suite stays green.
- **T5 deliberately collides with T3** (both target `growth_percent`). It is
  the control: a real conflict must be *detected at definition granularity*,
  not silently merged, and must not disturb T1/T2/T4.

Merging is done by `weftc splice` (built for this test): it parses base and
patch as ASTs and replaces/appends by definition name, reporting a conflict
when two patches touch the same name. Definition-level merge is a compiler
operation here, not a text diff.

## Tasks (fixed 2026-08-20, before any slice was generated)

**T1 — suppliers.** Targets `reorder_qty`, `low_stock_skus`. Out-of-stock
skus are urgent: when a sku's on-hand quantity is zero, reorder **double**
the normal quantity. Add `urgent_skus(inv, points) -> List[Text]` listing
skus at zero stock. Non-zero stock keeps its current quantity exactly.

**T2 — bundles.** Targets `savings_or_zero`, `bundle_label`, `best_bundle`.
A bundle is only worth offering when it saves at least 200 cents. Add
`bundle_worth_offering(b) -> Bool`; `best_bundle` must only ever return a
bundle worth offering; `bundle_label` must mark a thin bundle as such.

**T3 — analytics.** Targets `growth_percent`, `growth_series`. Explosive
growth is capped: `growth_percent` never reports more than 999. Add
`best_month(ms: List[MonthOrders]) -> Option[MonthRevenue]` returning the
highest-revenue month (ties keep the earlier month).

**T4 — gift cards.** Targets `card_spend`, `card_covers`. A gift card may
not be used on totals under 500 cents: `card_spend` pays 0 there and
`card_covers` is false. Add `card_usable(card, total) -> Bool`.

**T5 — analytics rounding (COLLISION CONTROL).** Targets `growth_percent`,
`growth_label`. Growth is reported in round numbers: `growth_percent` rounds
its result toward zero to the nearest multiple of 5, and `growth_label`
renders "about" before rounded values. Collides with T3 on `growth_percent`.

## Metrics

- Do T1–T4 splice cleanly and keep all 243+ tests green?
- Is the T3/T5 collision detected by name (not by file), and does it leave
  the disjoint patches untouched?
- Wall-clock: five concurrent agents vs. the same work in sequence.

## Results — 2026-08-20: the claim holds

**T1–T4 merged clean on the first splice: `weftc check` ok (191 defs, 262
tests), `weftc test` 262/262.** Four agents edited one file concurrently,
each blind to the others, and produced 6 replaced definitions and 23 new
items with **zero conflicts and zero repair rounds**.

**The collision control behaved exactly as designed.** Splicing T3 + T5
failed before writing anything:

```
conflict: 1 definition(s) edited by more than one patch
  def growth_percent — touched by t3-patch and t5-patch
```

Detected by *name*, not by file or line: T5 collides with T3 on one
definition while T1, T2 and T4 — which edit the same file, some within a few
lines of the conflict — are entirely unaffected. That is the claim, and it
is now demonstrated rather than asserted.

**Wall clock**: the five agents ran concurrently in ~102s (the slowest one);
the same work in sequence would have been ~350s of agent time.

### The finding that mattered most

T3 was asked to add `best_month(ms) -> Option[MonthRevenue]`. It noticed from
the **whole-program signature map in its slice** that `best_month` already
existed — returning `Option[Int]`, defined outside its slice — and refused to
shadow it, delivering `best_month_revenue` instead with an explanation.

That is a real limitation of name-based merging: `weftc splice` catches
patch-vs-patch conflicts, but a single patch replacing a base definition with
one of a *different type* is not a conflict — it is a silent, breaking
replacement. Two defences now exist:

1. The slice's full signature map, which is how T3 caught it (an argument for
   never trimming the map out of a slice, however large the program).
2. **`weftc splice` now warns on any replacement that changes a definition's
   signature**, printing the old and new side by side — added in response to
   this finding, and verified against a synthetic type-changing patch.

### Honest scope

Definition-granularity merge is proven for *concurrent independent edits*.
It does not attempt semantic conflict detection: two patches that never touch
the same definition can still disagree about behaviour (e.g. both tightening
the same business rule from different ends). The test suite is what catches
that class, and it did not arise here.
