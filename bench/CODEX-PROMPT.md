# Prompt for Codex (GPT-5.6-Luna) — cross-model exit test, iteration 1

Paste everything below the line into Codex, run from d:\Dev2\unknown.

---

You are a test subject in a language-learnability experiment for a new
programming language called Weft. Weft has zero training data — you have never
seen it. The experiment measures whether you can write correct Weft using only
its specification. Follow these rules exactly; breaking them invalidates the
experiment.

**READ exactly one file:** `SPEC.md` (in the repo root) — the complete
specification of Weft, about four pages. It is your ONLY source of knowledge
about the language. Where Weft differs from languages you know, the spec wins.

**FORBIDDEN:** reading any other file in this repository — in particular
`examples/`, `exit-test/`, `bench/hidden/`, `bench/subs/` (except writing your
own answers as instructed below), `README.md`, `EXIT-TEST.md`, and `weftc/`.
Also forbidden: running any compiler, interpreter, or test tool on your
answers (including `weftc`). First-attempt quality is what is being measured;
you will receive real compiler feedback in a later round.

**TASK:** write 6 complete single-file Weft programs, one per task below.
Every program must: define exactly the signature given, include your own unit
tests (`test "name" = ...`) and a property test where natural, and end with
the entry point `def main(io: Io) -> Int = 0`. Write each program to the exact
path given (create `bench/subs/codex/` if needed).

1. `bench/subs/codex/collatz.weft`
   `def collatz_steps(n: Int where n >= 1) -> Int` — the number of Collatz
   steps to reach 1 from n. One step: even n becomes n/2, odd n becomes
   3*n+1. collatz_steps(1) is 0.

2. `bench/subs/codex/dedupe.weft`
   `def dedupe(xs: List[Int]) -> List[Int]` — remove consecutive duplicate
   integers, keeping the first element of each run. [1, 1, 2, 1] becomes
   [1, 2, 1].

3. `bench/subs/codex/luhn.weft`
   `def luhn(t: Text) -> Bool` — Luhn checksum validation. Valid if and only
   if: every character of t is a digit, t has at least 2 characters, and the
   Luhn sum is divisible by 10. Luhn sum: starting from the rightmost digit,
   double every second digit (positions 2, 4, ... counting from the right);
   if a doubled value exceeds 9, subtract 9; sum everything.

4. `bench/subs/codex/roman.weft`
   `def roman(n: Int where n >= 1 and n <= 3999) -> Text` — the Roman numeral
   for n in standard subtractive notation (4 is IV, 9 is IX, 40 is XL, 90 is
   XC, 400 is CD, 900 is CM).

5. `bench/subs/codex/mode_min.weft`
   `def mode_min(xs: List[Int] where len(xs) > 0) -> Int` — the most frequent
   value in xs; when several values tie for most frequent, return the
   smallest of them.

6. `bench/subs/codex/window_sum.weft`
   `def max_window_sum(xs: List[Int], k: Int where k >= 1) -> Option[Int]` —
   the maximum sum over any k consecutive elements of xs; None when xs has
   fewer than k elements.

When finished, reply with only the list of files you wrote — no explanations.

In a later round you may be shown compiler or test output for some of your
files. When that happens, rewrite the complete corrected file at the same
path — full file, not a patch.
