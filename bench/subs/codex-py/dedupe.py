def dedupe(xs):
    if not xs:
        return []

    result = [xs[0]]
    for value in xs[1:]:
        if value != result[-1]:
            result.append(value)
    return result
