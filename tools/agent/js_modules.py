"""Locate literal JavaScript module imports without treating strings as code."""
from __future__ import annotations


def may_reference_local_assets(source: str) -> bool:
    return ('import' in source or 'fetch' in source or 'Audio' in source
            or ('src' in source or 'setAttribute' in source) and ('Image' in source or 'createElement' in source))


def module_references(source: str) -> list[tuple[int, int, str, str]]:
    if 'import' not in source and 'export' not in source and not may_reference_local_assets(source):
        return []
    tokens: list[tuple[str, str, int, int]] = []
    n = len(source)

    def emit(kind: str, value: str, start: int, end: int) -> None:
        if len(tokens) >= 1_000_000: raise ValueError('JavaScript has too many tokens')
        tokens.append((kind, value, start, end))

    def template(at: int) -> int:
        start = at
        at += 1
        emit('template', '`', start, at)
        while at < n:
            if source[at] == '\\': at += 2; continue
            if source[at] == '`':
                emit('template', '`', at, at + 1)
                return at + 1
            if source.startswith('${', at):
                at = code(at + 2, True)
                continue
            at += 1
        raise ValueError('unterminated JavaScript template')

    def code(at: int, stop_brace: bool = False) -> int:
        depth = 0
        parens: list[bool] = []
        while at < n:
            ch = source[at]
            if ch.isspace(): at += 1; continue
            if source.startswith('//', at):
                end = source.find('\n', at + 2)
                at = n if end < 0 else end + 1
                continue
            if source.startswith('/*', at):
                end = source.find('*/', at + 2)
                if end < 0: raise ValueError('unterminated JavaScript comment')
                at = end + 2; continue
            if ch in "\"'":
                start, quote = at, ch
                at += 1
                while at < n:
                    if source[at] == '\\': at += 2; continue
                    if source[at] == quote: break
                    if source[at] in '\r\n': raise ValueError('unterminated JavaScript string')
                    at += 1
                if at >= n: raise ValueError('unterminated JavaScript string')
                emit('string', source[start + 1:at], start, at + 1)
                at += 1; continue
            if ch == '`': at = template(at); continue
            if ch == '}' and stop_brace and depth == 0: return at + 1
            if ch == '{': depth += 1
            elif ch == '}' and depth: depth -= 1
            if ch == '/':
                previous = tokens[-1] if tokens else None
                regex_start = previous is None or previous[0] == 'control_close' or previous[0] == 'punct' and previous[1] not in (')', ']', '}', '++', '--') or previous[0] == 'word' and previous[1] in ('return', 'throw', 'case', 'delete', 'void', 'typeof', 'new', 'yield', 'await', 'in', 'of')
                if regex_start:
                    start, at, in_class = at, at + 1, False
                    while at < n:
                        if source[at] == '\\': at += 2; continue
                        if source[at] == '[': in_class = True
                        elif source[at] == ']': in_class = False
                        elif source[at] == '/' and not in_class: break
                        if source[at] in '\r\n': raise ValueError('unterminated JavaScript regular expression')
                        at += 1
                    if at >= n: raise ValueError('unterminated JavaScript regular expression')
                    at += 1
                    while at < n and (source[at].isalnum() or source[at] in '_$'): at += 1
                    emit('regex', source[start:at], start, at)
                    continue
            if ch.isalpha() or ch in '_$' or ord(ch) >= 128:
                start = at; at += 1
                while at < n and (source[at].isalnum() or source[at] in '_$' or ord(source[at]) >= 128): at += 1
                emit('word', source[start:at], start, at)
                continue
            if ch.isdigit():
                start = at; at += 1
                while at < n and (source[at].isalnum() or source[at] in '._'): at += 1
                emit('number', source[start:at], start, at)
                continue
            punct = next((p for p in ('?.', '++', '--', '=>') if source.startswith(p, at)), ch)
            previous = tokens[-1] if tokens else None
            if punct == '(':
                parens.append(bool(previous and previous[0] == 'word' and previous[1] in ('if', 'while', 'for', 'with', 'switch', 'catch')))
            control_close = punct == ')' and bool(parens and parens.pop())
            emit('control_close' if control_close else 'punct', punct, at, at + len(punct)); at += len(punct)
        if stop_brace: raise ValueError('unterminated JavaScript template expression')
        return at

    code(0)
    scopes: list[int] = []
    stack = [0]
    next_scope = 0
    for _, value, _, _ in tokens:
        if value == '}' and len(stack) > 1: stack.pop()
        scopes.append(stack[-1])
        if value == '{':
            next_scope += 1
            stack.append(next_scope)
    builtins = {'fetch', 'Image', 'Audio', 'document', 'window', 'self', 'globalThis'}
    shadowed = set()
    binding_steps = 0
    for i, (_, word, _, _) in enumerate(tokens):
        if word not in ('const', 'let', 'var', 'import'): continue
        start = i + 1
        if (word == 'import' and start + 2 < len(tokens) and tokens[start][1] == '*'
                and tokens[start + 1][1] == 'as' and tokens[start + 2][0] == 'word' and tokens[start + 2][1] in builtins):
            shadowed.add(tokens[start + 2][1])
        if word == 'import' and start + 1 < len(tokens) and tokens[start][0] == 'word' and tokens[start + 1][1] == ',': start += 2
        if start >= len(tokens) or tokens[start][1] not in ('{', '['): continue
        opening = tokens[start][1]
        closing = '}' if opening == '{' else ']'
        nesting, closed = 0, False
        for kind, value, _, _ in tokens[start:start + 4096]:
            binding_steps += 1
            if binding_steps > 2_000_000: break
            if value == opening: nesting += 1
            if value == closing:
                nesting -= 1
                if nesting == 0:
                    closed = True
                    break
            if kind == 'word' and value in builtins: shadowed.add(value)
        if not closed: shadowed.update(builtins)
        if binding_steps > 2_000_000: break
    for i, (kind, name, _, _) in enumerate(tokens):
        if kind != 'word' or name not in builtins: continue
        declaration = (i > 0 and (tokens[i - 1][1] in ('const', 'let', 'var', 'function', 'class', 'import')
                       or tokens[i - 1][1] == ',' and i + 1 < len(tokens) and tokens[i + 1][1] == '='))
        parameter = (i + 1 < len(tokens) and (tokens[i + 1][1] == '=>'
                     or i > 0 and tokens[i - 1][1] in ('(', ',') and tokens[i + 1][1] in (')', ',', '=')))
        if declaration or parameter: shadowed.add(name)
    parameter_steps = 0
    for i, (kind, word, _, _) in enumerate(tokens):
        if kind != 'word' or word not in ('function', 'catch'): continue
        start = next((j for j in range(i + 1, min(len(tokens), i + 32)) if tokens[j][1] == '('), None)
        if start is None: continue
        nesting, closed = 0, False
        for token_kind, value, _, _ in tokens[start:start + 4096]:
            parameter_steps += 1
            if parameter_steps > 2_000_000: break
            if value == '(': nesting += 1
            if value == ')':
                nesting -= 1
                if nesting == 0:
                    closed = True
                    break
            if token_kind == 'word' and value in builtins: shadowed.add(value)
        if not closed: shadowed.update(builtins)
        if parameter_steps > 2_000_000: break
    elements: dict[str, tuple[int, int]] = {}
    declarations: dict[str, int] = {}
    initializer_steps = 0
    def ends_initializer(end: int) -> bool:
        if end + 1 >= len(tokens): return True
        following = tokens[end + 1]
        return (following[1] in (';', ',', ')', '}') or following[0] == 'word'
                and any(ch in source[tokens[end][3]:following[2]] for ch in '\r\n'))
    for i, (kind, word, _, _) in enumerate(tokens):
        if kind != 'word' or word not in ('const', 'let', 'var') or i + 1 >= len(tokens) or tokens[i + 1][0] != 'word': continue
        name = tokens[i + 1][1]
        declarations[name] = declarations.get(name, 0) + 1
        rest = [token[1] for token in tokens[i + 2:i + 9]]
        constructor = rest[:2] == ['=', 'new'] and len(rest) >= 3 and rest[2] in ('Image', 'Audio') and rest[2] not in shadowed
        if constructor and i + 5 < len(tokens) and tokens[i + 5][1] == '(':
            nesting, end = 0, None
            for j in range(i + 5, min(len(tokens), i + 261)):
                initializer_steps += 1
                if initializer_steps > 2_000_000: break
                if tokens[j][1] == '(': nesting += 1
                if tokens[j][1] == ')':
                    nesting -= 1
                    if nesting == 0:
                        end = j
                        break
            constructor = end is not None and ends_initializer(end)
        elif constructor:
            constructor = ends_initializer(i + 4)
        created = ('document' not in shadowed and len(rest) >= 7 and rest[:5] == ['=', 'document', '.', 'createElement', '(']
                   and tokens[i + 7][0] == 'string' and rest[5].lower() in ('img', 'audio', 'video', 'source') and rest[6] == ')'
                   and ends_initializer(i + 8))
        constructed = constructor or created
        if constructed and word == 'const': elements[name] = (i, scopes[i])
    elements = {name: item for name, item in elements.items() if declarations[name] == 1}
    arrow_steps = 0
    for i, (_, value, _, _) in enumerate(tokens):
        if value != '=>' or i == 0: continue
        if tokens[i - 1][0] == 'word':
            name = tokens[i - 1][1]
            elements.pop(name, None)
            if name in builtins: shadowed.add(name)
        elif tokens[i - 1][1] == ')':
            nesting, start = 0, None
            for j in range(i - 1, -1, -1):
                arrow_steps += 1
                if arrow_steps > 2_000_000:
                    elements.clear()
                    break
                if tokens[j][1] == ')': nesting += 1
                elif tokens[j][1] == '(':
                    nesting -= 1
                    if nesting == 0:
                        start = j
                        break
            if start is None:
                elements.clear()
                break
            for kind, name, _, _ in tokens[start + 1:i - 1]:
                if kind == 'word':
                    elements.pop(name, None)
                    if name in builtins: shadowed.add(name)
    if shadowed.intersection(('Image', 'Audio', 'document')): elements.clear()
    result = []
    for i, (kind, word, _, _) in enumerate(tokens):
        if kind == 'word' and word in elements and elements[word][0] < i and elements[word][1] == scopes[i] and (i == 0 or tokens[i - 1][1] not in ('.', '?.')) and i + 4 < len(tokens) and tokens[i + 1][1] == '.':
            assignment = (tokens[i + 2][1] == 'src' and tokens[i + 3][1] == '=' and
                          (i + 5 == len(tokens) or tokens[i + 5][1] in (';', ')', ',', '}')))
            attribute = (i + 7 < len(tokens) and tokens[i + 2][1] == 'setAttribute' and tokens[i + 3][1] == '('
                         and tokens[i + 4][0] == 'string' and tokens[i + 4][1].lower() == 'src'
                         and tokens[i + 5][1] == ',' and tokens[i + 7][1] == ')')
            position = i + 4 if assignment else i + 6 if attribute else None
            if position is not None and tokens[position][0] == 'string':
                _, value, begin, end = tokens[position]
                if '\\' in value: raise ValueError('escaped JavaScript media paths are not supported')
                result.append((begin, end, value, 'dom'))
        if (kind == 'word' and word == 'new' and 'Audio' not in shadowed and i + 4 < len(tokens)
                and [token[1] for token in tokens[i + 1:i + 3]] == ['Audio', '(']
                and tokens[i + 3][0] == 'string' and tokens[i + 4][1] == ')'):
            _, value, begin, end = tokens[i + 3]
            if '\\' in value: raise ValueError('escaped JavaScript media paths are not supported')
            result.append((begin, end, value, 'dom'))
        if i == 0 or tokens[i - 1][1] not in ('.', '?.'):
            global_fetch = 'fetch' not in shadowed
        else:
            global_fetch = (i >= 2 and tokens[i - 2][1] in ('window', 'globalThis', 'self')
                            and tokens[i - 2][1] not in shadowed and (i == 2 or tokens[i - 3][1] not in ('.', '?.')))
        if (kind == 'word' and word == 'fetch' and global_fetch and i + 3 < len(tokens)
                and tokens[i + 1][1] == '(' and tokens[i + 2][0] == 'string' and tokens[i + 3][1] in (')', ',')):
            _, value, begin, end = tokens[i + 2]
            if '\\' in value: raise ValueError('escaped JavaScript fetch paths are not supported')
            result.append((begin, end, value, 'fetch'))
        if (kind == 'word' and word == 'new' and i + 10 < len(tokens)
                and [token[1] for token in tokens[i + 1:i + 3]] == ['URL', '(']
                and tokens[i + 3][0] == 'string'
                and [token[1] for token in tokens[i + 4:i + 11]] == [',', 'import', '.', 'meta', '.', 'url', ')']):
            _, value, begin, end = tokens[i + 3]
            if '\\' in value: raise ValueError('escaped JavaScript resource paths are not supported')
            result.append((begin, end, value, 'resource'))
        if kind != 'word' or word not in ('import', 'export') or i and tokens[i - 1][1] in ('.', '?.'): continue
        start = i + 1
        if start >= len(tokens): continue
        token = tokens[start]
        position = None
        if word == 'import' and token[0] == 'string': position = start
        elif word == 'import' and token[1] == '(':
            if start + 2 < len(tokens) and tokens[start + 1][0] == 'string' and tokens[start + 2][1] in (')', ','): position = start + 1
        elif word == 'export' and token[1] not in ('{', '*'): continue
        elif word == 'import' and token[0] != 'word' and token[1] not in ('{', '*'): continue
        else:
            for j in range(start, min(len(tokens), start + 4096)):
                if tokens[j][1] in (';', 'import', 'export'): break
                if tokens[j][1] == 'from' and j + 1 < len(tokens) and tokens[j + 1][0] == 'string': position = j + 1; break
        if position is not None:
            _, value, begin, end = tokens[position]
            if '\\' in value: raise ValueError('escaped JavaScript module paths are not supported')
            result.append((begin, end, value, 'module'))
    return result


def module_imports(source: str) -> list[tuple[int, int, str]]:
    return [(start, end, value) for start, end, value, kind in module_references(source) if kind == 'module']
