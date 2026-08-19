def parse_csv(t):
    if t == "":
        return []
    rows = []
    row = []
    buf = []
    state = "start"  # start | unquoted | quoted | after
    i = 0
    n = len(t)
    while i < n:
        c = t[i]
        if state == "start":
            if c == '"':
                state = "quoted"
            elif c == ",":
                row.append("")
            elif c == "\n":
                row.append("")
                rows.append(row)
                row = []
            else:
                buf.append(c)
                state = "unquoted"
        elif state == "unquoted":
            if c == '"':
                raise ValueError("unexpected quote inside unquoted field")
            elif c == ",":
                row.append("".join(buf))
                buf = []
                state = "start"
            elif c == "\n":
                row.append("".join(buf))
                buf = []
                rows.append(row)
                row = []
                state = "start"
            else:
                buf.append(c)
        elif state == "quoted":
            if c == '"':
                if i + 1 < n and t[i + 1] == '"':
                    buf.append('"')
                    i += 1
                else:
                    state = "after"
            else:
                buf.append(c)
        else:  # state == "after"
            if c == ",":
                row.append("".join(buf))
                buf = []
                state = "start"
            elif c == "\n":
                row.append("".join(buf))
                buf = []
                rows.append(row)
                row = []
                state = "start"
            else:
                raise ValueError("closing quote must be followed by comma, newline, or end of input")
        i += 1
    if state == "quoted":
        raise ValueError("unterminated quote in field")
    if state == "unquoted" or state == "after":
        row.append("".join(buf))
        rows.append(row)
    elif state == "start" and row:
        row.append("")
        rows.append(row)
    return rows
