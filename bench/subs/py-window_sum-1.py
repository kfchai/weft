def max_window_sum(xs, k):
    if k < 1 or len(xs) < k:
        return None
    current = sum(xs[:k])
    best = current
    for i in range(k, len(xs)):
        current += xs[i] - xs[i - k]
        if current > best:
            best = current
    return best
