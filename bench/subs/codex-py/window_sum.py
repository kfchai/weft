def max_window_sum(xs, k):
    if len(xs) < k:
        return None

    current = sum(xs[:k])
    best = current
    for index in range(k, len(xs)):
        current += xs[index] - xs[index - k]
        if current > best:
            best = current
    return best
