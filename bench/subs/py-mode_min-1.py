def mode_min(xs):
    counts = {}
    for x in xs:
        counts[x] = counts.get(x, 0) + 1
    best = None
    best_count = 0
    for value, count in counts.items():
        if count > best_count or (count == best_count and value < best):
            best = value
            best_count = count
    return best
