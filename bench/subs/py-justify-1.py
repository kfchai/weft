def justify(words, width):
    lines = []
    i = 0
    n = len(words)
    groups = []
    while i < n:
        j = i
        length = len(words[i])
        j += 1
        while j < n and length + 1 + len(words[j]) <= width:
            length += 1 + len(words[j])
            j += 1
        groups.append(words[i:j])
        i = j
    for gi, group in enumerate(groups):
        if gi == len(groups) - 1 or len(group) == 1:
            line = " ".join(group)
            line += " " * (width - len(line))
            lines.append(line)
        else:
            total_chars = sum(len(w) for w in group)
            gaps = len(group) - 1
            spaces = width - total_chars
            base, extra = divmod(spaces, gaps)
            parts = []
            for k, w in enumerate(group[:-1]):
                parts.append(w)
                parts.append(" " * (base + (1 if k < extra else 0)))
            parts.append(group[-1])
            lines.append("".join(parts))
    return lines
