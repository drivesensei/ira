"""Rust token masking and literal metadata inspection for source isolation."""
import re

def rust_code(source, literals=None):
    """Mask strings/comments, preserving positions; nested block comments are supported."""
    chars = list(source)
    raw_pattern = re.compile(r'(?:br|cr|r)(#{0,255})"')
    i = 0
    while i < len(source):
        end = i
        literal = False
        if source.startswith("//", i):
            end = source.find("\n", i)
            if end < 0: end = len(source)
        elif source.startswith("/*", i):
            end, depth = i + 2, 1
            while end < len(source) and depth:
                if source.startswith("/*", end): depth += 1; end += 2
                elif source.startswith("*/", end): depth -= 1; end += 2
                else: end += 1
        else:
            raw = raw_pattern.match(source, i)
            if raw:
                literal = source.startswith("r", i)
                stop = source.find('"' + raw[1], raw.end())
                end = len(source) if stop < 0 else stop + 1 + len(raw[1])
            elif source[i] == '"':
                literal = True
                end = i + 1
                while end < len(source):
                    if source[end] == "\\": end += 2
                    elif source[end] == '"': end += 1; break
                    else: end += 1
            elif source[i] == "'":
                char = re.compile(r"'(?:\\.|[^'\\])'").match(source, i)
                if char: end = char.end()
        if end > i:
            if literal and literals is not None: literals[i] = (end, source[i:end])
            for j in range(i, min(end, len(chars))):
                if chars[j] != "\n": chars[j] = " "
            i = end
        else: i += 1
    return "".join(chars)



def string_value(literal):
    """Decode Rust string escapes (not Python escapes) before resolving paths."""
    if literal.startswith("r"):
        opening=re.match(r'r(#{0,255})"',literal)
        return literal[opening.end():len(literal)-1-len(opening[1])]
    value=literal[1:-1]
    pattern=r"\\(?:x([0-9a-fA-F]{2})|u\{([0-9a-fA-F_]+)\}|([nrt0\\'\"])|\r?\n[ \t\r\n]*)"
    def unescape(match):
        if match[1]: return chr(int(match[1],16))
        if match[2]: return chr(int(match[2].replace("_",""),16))
        if match[3]: return {"n":"\n","r":"\r","t":"\t","0":"\0"}.get(match[3],match[3])
        return ""
    return re.sub(pattern,unescape,value)


def module_paths(source):
    """Yield module path metadata, including nested cfg_attr; ignore decoy text.

    Conditional paths are checked for all target configurations, as dependencies
    are. The predicate itself is never confused with a module path attribute.
    """
    literals={};code=rust_code(source,literals)
    def metadata(begin,end):
        match=re.match(r"\s*(path|cfg_attr)\b",code[begin:end])
        if not match:return
        cursor=begin+match.end()
        if match[1]=="path":
            equal=re.match(r"\s*=",code[cursor:end])
            if equal:
                value_begin=cursor+equal.end()
                for start,(stop,literal) in literals.items():
                    if value_begin<=start<stop<=end and not code[value_begin:start].strip():
                        yield string_value(literal);break
        else:
            opening=re.match(r"\s*\(",code[cursor:end])
            if not opening:return
            start=cursor+opening.end();depth=0;arguments=[]
            for i in range(start,end):
                char=code[i]
                if char in "([{":depth+=1
                elif char in ")]}":
                    if depth==0:
                        arguments.append((start,i));break
                    depth-=1
                elif char=="," and depth==0:
                    arguments.append((start,i));start=i+1
            for a,b in arguments[1:]:yield from metadata(a,b)
    for match in re.finditer(r"#\s*!?\s*\[",code):
        begin=match.end();depth=1
        for end in range(begin,len(code)):
            if code[end]=="[":depth+=1
            elif code[end]=="]":
                depth-=1
                if depth==0:
                    yield from metadata(begin,end);break
