def mode_min(xs):
    counts = {}
    for value in xs:
        counts[value] = counts.get(value, 0) + 1
    highest = max(counts.values())
    return min(value for value, count in counts.items() if count == highest)
