def luhn(t):
    if len(t) < 2 or not t.isdigit():
        return False

    total = 0
    for position, character in enumerate(reversed(t)):
        digit = ord(character) - ord("0")
        if position % 2 == 1:
            digit *= 2
            if digit > 9:
                digit -= 9
        total += digit
    return total % 10 == 0
