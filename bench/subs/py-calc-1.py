def calc(t):
    tokens = _tokenize(t)
    pos = [0]
    value = _parse_expr(tokens, pos)
    if pos[0] != len(tokens):
        raise ValueError("syntax error: unexpected trailing input")
    return value


def _tokenize(t):
    if not isinstance(t, str):
        raise ValueError("syntax error: input must be a string")
    tokens = []
    i = 0
    n = len(t)
    while i < n:
        c = t[i]
        if c in " \t\r\n":
            i += 1
        elif c.isdigit():
            j = i
            while j < n and t[j].isdigit():
                j += 1
            tokens.append(("num", int(t[i:j])))
            i = j
        elif c in "+-*/()":
            tokens.append((c, c))
            i += 1
        else:
            raise ValueError("syntax error: unexpected character %r" % c)
    if not tokens:
        raise ValueError("syntax error: empty expression")
    return tokens


def _peek(tokens, pos):
    if pos[0] < len(tokens):
        return tokens[pos[0]][0]
    return None


def _parse_expr(tokens, pos):
    value = _parse_term(tokens, pos)
    while _peek(tokens, pos) in ("+", "-"):
        op = tokens[pos[0]][0]
        pos[0] += 1
        rhs = _parse_term(tokens, pos)
        if op == "+":
            value = value + rhs
        else:
            value = value - rhs
    return value


def _parse_term(tokens, pos):
    value = _parse_unary(tokens, pos)
    while _peek(tokens, pos) in ("*", "/"):
        op = tokens[pos[0]][0]
        pos[0] += 1
        rhs = _parse_unary(tokens, pos)
        if op == "*":
            value = value * rhs
        else:
            if rhs == 0:
                raise ValueError("division by zero")
            q = abs(value) // abs(rhs)
            if (value < 0) != (rhs < 0):
                q = -q
            value = q
    return value


def _parse_unary(tokens, pos):
    if _peek(tokens, pos) == "-":
        pos[0] += 1
        return -_parse_unary(tokens, pos)
    return _parse_atom(tokens, pos)


def _parse_atom(tokens, pos):
    kind = _peek(tokens, pos)
    if kind == "num":
        value = tokens[pos[0]][1]
        pos[0] += 1
        return value
    if kind == "(":
        pos[0] += 1
        value = _parse_expr(tokens, pos)
        if _peek(tokens, pos) != ")":
            raise ValueError("syntax error: expected closing parenthesis")
        pos[0] += 1
        return value
    raise ValueError("syntax error: expected a value")
