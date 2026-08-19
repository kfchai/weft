# Prompt for Codex (GPT-5.6-Luna) — Python control arm

IMPORTANT: run this in a **fresh Codex session** (Luna already solved these
tasks in Weft; a shared session would contaminate the control). Paste
everything below the line, run from d:\Dev2\unknown.

---

You are a test subject in a programming benchmark. Follow these rules exactly.

**FORBIDDEN:** reading any file in this repository, and running any
interpreter, compiler, or test tool on your answers (including python).
First-attempt quality is what is being measured; you will receive real test
output in a later round. Write from knowledge alone.

**TASK:** write 6 single-file Python 3 solutions, standard library only, one
per task below. Define the exact function signature given at top level (the
grader appends asserts to your file). You may include your own tests only
under `if __name__ == "__main__":`. Write each file to the exact path given
(create `bench/subs/codex-py/` if needed).

1. `bench/subs/codex-py/collatz.py`
   `def collatz_steps(n):` — the number of Collatz steps to reach 1 from n
   (n >= 1). One step: even n becomes n // 2, odd n becomes 3*n+1.
   collatz_steps(1) is 0.

2. `bench/subs/codex-py/dedupe.py`
   `def dedupe(xs):` — remove consecutive duplicate integers from the list,
   keeping the first element of each run. [1, 1, 2, 1] becomes [1, 2, 1].

3. `bench/subs/codex-py/luhn.py`
   `def luhn(t):` — Luhn checksum validation of a string, returning
   True/False. Valid if and only if: every character of t is a digit, t has
   at least 2 characters, and the Luhn sum is divisible by 10. Luhn sum:
   starting from the rightmost digit, double every second digit (positions
   2, 4, ... counting from the right); if a doubled value exceeds 9,
   subtract 9; sum everything.

4. `bench/subs/codex-py/roman.py`
   `def roman(n):` — the Roman numeral for n (1 <= n <= 3999) in standard
   subtractive notation (4 is IV, 9 is IX, 40 is XL, 90 is XC, 400 is CD,
   900 is CM).

5. `bench/subs/codex-py/mode_min.py`
   `def mode_min(xs):` — the most frequent value in the non-empty list xs;
   when several values tie for most frequent, return the smallest of them.

6. `bench/subs/codex-py/window_sum.py`
   `def max_window_sum(xs, k):` — the maximum sum over any k consecutive
   elements of the list xs (k >= 1); return None when xs has fewer than k
   elements.

When finished, reply with only the list of files you wrote — no explanations.

In a later round you may be shown test output for some of your files. When
that happens, rewrite the complete corrected file at the same path — full
file, not a patch.
