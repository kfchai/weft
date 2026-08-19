def luhn(t):
    if not isinstance(t, str) or len(t) < 2 or not t.isdigit():
        return False
    total = 0
    for i, ch in enumerate(reversed(t)):
        d = int(ch)
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0
