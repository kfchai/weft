def book(existing, start, end):
    if end <= start:
        raise ValueError("invalid interval: end must be greater than start")
    for s, e in existing:
        if start < e and s < end:
            raise ValueError("overlap with existing slot ({}, {})".format(s, e))
    slots = sorted(list(existing) + [(start, end)])
    merged = []
    for s, e in slots:
        if merged and merged[-1][1] == s:
            merged[-1] = (merged[-1][0], e)
        else:
            merged.append((s, e))
    return [tuple(iv) for iv in merged]
