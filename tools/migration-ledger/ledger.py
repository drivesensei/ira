#!/usr/bin/env python3
"""
ledger.py — the shared gap ledger for the dev-team skill (v2: indexed, sharded).

Every finding lives in its OWN small file, grouped by area (feature, module or
cell), and the ledger keeps short indexes up to date automatically after every
write. Agents read an index first and open only the gap files they act on, so
reading cost stays flat no matter how many gaps a migration produces.

  .devteam/gaps/
    INDEX.md                 top index: one row per area + "needs attention" (auto, capped)
    index.json               compact machine index used by list/gate (auto; never read it)
    <area>/INDEX.md          one line per OPEN gap in that area, linking to its file (auto)
    <area>/RESOLVED.md       one line per resolved gap (auto archive)
    <area>/G-0042.md         the gap itself: metadata + short sections (written by this CLI)

Size limits are enforced so no file can balloon: long logs, traces, probe
output and analysis go to .devteam/evidence/... and are LINKED (--link), never
pasted.

Workspace resolution: --dir PATH  >  $DEVTEAM_DIR  >  nearest .devteam/ upward.

Commands:
  init [--max-agents N]
  add --by ID --area AREA --severity S --category C --title T --desc D
      [--task T-..] [--location file:line] [--evidence "short repro"] [--link PATH]...
      [--expected ..] [--suggest ..] [--confidence high|medium|low] [--phase ..]
  assign ID --to OWNER --by ID           submit ID --by ID (--evidence ..|--link ..) [--note]
  verify ID --by ID --note ..            reopen ID --by ID --note ..
  dispute ID --by ID --note ..           comment ID --by ID --note ..
  link ID --by ID --link PATH [--link PATH]...     attach evidence files
  move ID --by ID --area NEW_AREA
  rule ID --by ID --decision uphold|wont_fix|withdraw|defer|duplicate --note .. [--human-approved]
  list [--open] [--area A] [--status ..] [--severity ..] [--owner ..] [--task ..] [--limit N] [--json]
  show ID                                prints the gap file (small by construction)
  archive-history ID --by ID --expected-sha256 SHA [--keep 3]
                                         preserve exact bytes and full history, shorten active file
  path ID                                prints the gap file's path
  gate [--phase design|review|done] [--area A]
  stats [--area A]
  render                                 rebuild all markdown indexes
  reindex                                rebuild index.json from the gap files
  doctor                                 find oversized/stray files, broken gap files
  migrate                                convert a v1 .devteam/gaps.json into gap files
"""
import argparse
import base64
import contextlib
import datetime as dt
import json
import hashlib
import os
import re
import shutil
import stat
import sys
import tempfile

try:
    import fcntl
except ImportError:  # pragma: no cover
    fcntl = None

SEVERITIES = ["blocker", "major", "minor", "nit"]
CATEGORIES = [
    "logic", "edge-case", "test-gap", "unproven-claim", "design", "security",
    "performance", "maintainability", "spec-ambiguity", "integration", "ux", "docs",
]
STATUSES = ["open", "assigned", "fix_submitted", "disputed", "closed",
            "wont_fix", "deferred", "duplicate"]
RESOLVED = {"closed", "wont_fix", "duplicate"}
ESCALATE_AFTER_ROUNDS = 3

# size limits (characters) — keep every gap file a few KB at most
MAX_TITLE = 90
MAX_DESC = 1200
MAX_SHORT = 600        # evidence (inline), expected, suggestion, fix evidence
MAX_NOTE = 300         # history notes, comments, rulings
MAX_LINKS = 12
MAX_HISTORY = 40       # older entries are compacted into one summary line
INDEX_TITLE = 70       # titles are truncated to this in index rows
ATTENTION_ROWS = 25    # cap on rows in the top index "needs attention" section
AREA_INDEX_ROWS = 120  # cap on rows in an area index (highest severity first); split big areas
DOCTOR_MD_LIMIT = 12_000
DOCTOR_GAP_LIMIT = 5_000

TRANSITIONS = {
    "assign":  ({"open", "disputed", "deferred"}, "assigned"),
    "submit":  ({"assigned", "open"}, "fix_submitted"),
    "verify":  ({"fix_submitted"}, "closed"),
    "reopen":  ({"fix_submitted", "closed", "wont_fix", "deferred"}, "open"),
    "dispute": ({"open", "assigned", "fix_submitted"}, "disputed"),
}
RULE_DECISIONS = {
    "uphold":    ({"disputed"}, "assigned"),
    "wont_fix":  ({"open", "assigned", "disputed", "deferred"}, "wont_fix"),
    "withdraw":  ({"open", "assigned", "disputed"}, "closed"),
    "defer":     ({"open", "assigned", "disputed"}, "deferred"),
    "duplicate": ({"open", "assigned", "disputed"}, "duplicate"),
}
META_KEYS = ["id", "title", "severity", "category", "status", "area", "task", "location",
             "phase", "raised_by", "owner", "confidence", "rounds", "verified_by",
             "created", "updated"]
SECTIONS = [("description", "Description"), ("evidence", "Evidence"), ("links", "Links"),
            ("expected", "Expected"), ("suggestion", "Suggestion"),
            ("fix_evidence", "Fix evidence"), ("resolution", "Resolution"),
            ("history", "History")]
SUMMARY_KEYS = ["id", "title", "severity", "category", "status", "area", "task", "owner",
                "raised_by", "phase", "rounds", "updated"]
SLUG = re.compile(r"^[a-z0-9][a-z0-9_-]{0,39}$")
ACTOR = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,49}$")


# ------------------------------------------------------------------ helpers
def now():
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def die(msg, code=2):
    print(f"ledger: {msg}", file=sys.stderr)
    sys.exit(code)


def find_dir(explicit):
    if explicit:
        return os.path.abspath(explicit)
    if os.environ.get("DEVTEAM_DIR"):
        return os.path.abspath(os.environ["DEVTEAM_DIR"])
    cur = os.getcwd()
    while True:
        cand = os.path.join(cur, ".devteam")
        if os.path.isdir(cand):
            return cand
        parent = os.path.dirname(cur)
        if parent == cur:
            return os.path.join(os.getcwd(), ".devteam")
        cur = parent


def norm_id(gid):
    m = re.match(r"^[Gg]-?0*(\d+)$", gid.strip())
    if not m:
        die(f"bad gap id '{gid}' (expected G-0042)")
    return f"G-{int(m.group(1)):04d}"


def check_len(name, val, limit, hint=""):
    if val and len(val) > limit:
        die(f"--{name} is {len(val)} chars (limit {limit}). {hint or 'Shorten it.'}")


LONG_HINT = ("Keep the gap file short: write the long content (logs, traces, analysis, "
             "probe output) to $DEVTEAM_DIR/evidence/<task>/<name>.md|txt and attach it "
             "with --link <path>.")


def check_actor(x, what="--by"):
    if not x or not ACTOR.match(x):
        die(f"{what} '{x}' must be an agent id without spaces (e.g. reviewer-2, C3-qa)")


def atomic_write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".tmp.")
    with os.fdopen(fd, "w") as f:
        f.write(text)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)
    sync_directory(os.path.dirname(path))


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def gaps_root(ws):
    return os.path.join(ws, "gaps")


# -------------------------------------------------------- gap file (de)serialize
def gap_path(ws, g):
    return os.path.join(gaps_root(ws), g["area"], f"{g['id']}.md")


def one_line(s):
    return re.sub(r"\s+", " ", s or "").strip()


def serialize(g):
    out = ["---"]
    for k in META_KEYS:
        out.append(f"{k}: {json.dumps(g.get(k))}")
    if g.get("history_archive"):
        out.append(f"history_archive: {json.dumps(g['history_archive'], separators=(',', ':'))}")
    out += ["---", f"# {g['id']} — {g['title']}", "",
            "<!-- Written by ledger.py. Do not edit by hand; use the CLI. "
            "Long content belongs in evidence files listed under Links. -->", ""]
    for key, head in SECTIONS:
        val = g.get(key)
        if not val:
            continue
        out.append(f"## {head}")
        if key == "links":
            out += [f"- {p}" for p in val]
        elif key == "history":
            for h in val:
                note = f" — {one_line(h['note'])}" if h.get("note") else ""
                out.append(f"- {h['ts']} {h['by']} {h['action']}{note}")
        else:
            out.append(val.strip())
        out.append("")
    return "\n".join(out).rstrip() + "\n"


HIST_RE = re.compile(r"^- (\S+) (\S+) (\S+)(?: — (.*))?$")


def parse(text, path="?"):
    if not text.startswith("---\n"):
        raise ValueError(f"{path}: missing metadata block")
    end = text.find("\n---\n", 4)
    if end < 0:
        raise ValueError(f"{path}: unterminated metadata block")
    g = {}
    for line in text[4:end].splitlines():
        if not line.strip():
            continue
        k, _, v = line.partition(": ")
        g[k.strip()] = json.loads(v)
    body = text[end + 5:]
    heads = {h: k for k, h in SECTIONS}
    cur, buf = None, []

    def flush():
        if cur is None:
            return
        txt = "\n".join(buf).strip()
        if cur == "links":
            g["links"] = [l[2:].strip() for l in buf if l.startswith("- ")]
        elif cur == "history":
            hist = []
            for l in buf:
                m = HIST_RE.match(l.strip())
                if m:
                    hist.append({"ts": m.group(1), "by": m.group(2), "action": m.group(3),
                                 "note": m.group(4) or ""})
            g["history"] = hist
        else:
            g[cur] = txt
    for line in body.splitlines():
        if line.startswith("## ") and line[3:].strip() in heads:
            flush()
            cur, buf = heads[line[3:].strip()], []
        elif cur is not None:
            buf.append(line)
    flush()
    g.setdefault("history", [])
    g.setdefault("links", [])
    for k in META_KEYS:
        g.setdefault(k, None)
    g["rounds"] = g.get("rounds") or 0
    return g


def log(g, actor, action, note=""):
    g["history"].append({"ts": now(), "by": actor, "action": action, "note": one_line(note)[:MAX_NOTE]})
    if len(g["history"]) > MAX_HISTORY and not g.get("history_archive"):
        old = g["history"][:-(MAX_HISTORY - 1)]
        acts, total = {}, 0
        for h in old:
            if h["action"] == "compacted":       # fold a previous summary into this one
                for part in re.findall(r"(\S+)×(\d+)", h.get("note", "")):
                    acts[part[0]] = acts.get(part[0], 0) + int(part[1])
                    total += int(part[1])
                continue
            acts[h["action"]] = acts.get(h["action"], 0) + 1
            total += 1
        summ = ", ".join(f"{k}×{v}" for k, v in acts.items())
        g["history"] = [{"ts": old[-1]["ts"], "by": "ledger", "action": "compacted",
                         "note": f"{total} earlier events: {summ}"}] + g["history"][-(MAX_HISTORY - 1):]
    g["updated"] = now()


# Explicit, lossless history maintenance. The one active descriptor points to
# an immutable snapshot which includes its exact source bytes and prior chain.
ARCHIVE_PATH = re.compile(r'^evidence/ledger-history/G-\d{4,}/[0-9a-f]{64}\.json$')


def stable_bytes(path):
    fd = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0))
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f'{path}: not a regular file')
        with os.fdopen(fd, 'rb', closefd=False) as f:
            data = f.read()
        after = os.fstat(fd)
        fields = ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns')
        if any(getattr(before, k) != getattr(after, k) for k in fields) or len(data) != after.st_size:
            raise ValueError(f'{path}: changed during read')
        return data
    finally:
        os.close(fd)


def archive_path(ws, desc):
    rel = desc['path']
    if not isinstance(rel, str) or not ARCHIVE_PATH.fullmatch(rel):
        raise ValueError('invalid history archive path')
    path = os.path.join(ws, rel)
    # Refuse symlinked ancestor directories as well as a symlinked leaf.
    if os.path.realpath(path) != os.path.abspath(path):
        raise ValueError('history archive path contains a symlink')
    return path


def full_history(ws, g, seen=None):
    desc = g.get('history_archive')
    if not desc:
        return list(g['history'])
    seen = set() if seen is None else set(seen)
    path = archive_path(ws, desc)
    if path in seen:
        raise ValueError('history archive cycle')
    seen.add(path)
    raw = stable_bytes(path)
    digest = hashlib.sha256(raw).hexdigest()
    if len(raw) != desc['bytes'] or digest != desc['sha256'] or os.path.basename(path) != digest + '.json':
        raise ValueError('history archive bytes/hash mismatch')
    record = json.loads(raw)
    if record['version'] != 1 or record['id'] != g['id']:
        raise ValueError('history archive identity mismatch')
    source = base64.b64decode(record['source_base64'], validate=True)
    if hashlib.sha256(source).hexdigest() != record['source_sha256']:
        raise ValueError('history archive source hash mismatch')
    previous = parse(source.decode('utf-8'), path)
    if previous['id'] != g['id'] or full_history(ws, previous, seen) != record['history']:
        raise ValueError('history archive chronology mismatch')
    history, overlap = record['history'], desc['overlap']
    if (type(overlap) is not int or not 0 <= overlap <= len(history)
            or desc['events'] != len(history) or len(g['history']) < overlap
            or (overlap and g['history'][:overlap] != history[-overlap:])):
        raise ValueError('history archive retained tail mismatch')
    return history[:len(history) - overlap] + list(g['history'])


def archived_gap(ws, g, source, actor, keep):
    if parse(source.decode('utf-8')) != g:
        raise ValueError('archive source does not roundtrip to current gap')
    history = full_history(ws, g)
    if not 0 <= keep < len(g['history']):
        raise ValueError('--keep must leave at least one inline event to archive')
    record = {'version': 1, 'id': g['id'], 'by': actor,
              'source_sha256': hashlib.sha256(source).hexdigest(),
              'source_base64': base64.b64encode(source).decode('ascii'), 'history': history}
    raw = (json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(',', ':')) + '\n').encode()
    digest = hashlib.sha256(raw).hexdigest()
    desc = {'path': f"evidence/ledger-history/{g['id']}/{digest}.json",
            'sha256': digest, 'bytes': len(raw), 'events': len(history), 'overlap': keep}
    candidate = dict(g, history=history[-keep:] if keep else [], history_archive=desc)
    text = serialize(candidate)
    if len(text.encode()) > DOCTOR_GAP_LIMIT:
        raise ValueError('archived gap still exceeds unchanged 5000-byte limit')
    if parse(text) != candidate:
        raise ValueError('archive replacement does not roundtrip losslessly')
    path = archive_path(ws, desc)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    for directory in (os.path.dirname(path), os.path.dirname(os.path.dirname(path)),
                      os.path.join(ws, 'evidence'), ws):
        sync_directory(directory)
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o444)
    except FileExistsError:
        if stable_bytes(path) != raw:
            raise ValueError('existing immutable archive differs')
    else:
        with os.fdopen(fd, 'wb') as f:
            f.write(raw)
            f.flush()
            os.fsync(f.fileno())
        sync_directory(os.path.dirname(path))
    if full_history(ws, candidate) != history:
        raise ValueError('written archive lost history')
    return candidate


def cmd_archive_history(a):
    check_actor(a.by)
    if not re.fullmatch('[0-9a-f]{64}', a.expected_sha256):
        die('--expected-sha256 requires the exact source SHA256')
    if a.keep < 0:
        die('--keep must be nonnegative')
    try:
        with locked(a.ws) as st:
            g = load_gap(a.ws, st['index'], norm_id(a.id))
            path = gap_path(a.ws, g)
            source = stable_bytes(path)
            if hashlib.sha256(source).hexdigest() != a.expected_sha256:
                raise ValueError('source hash changed; refusing stale maintenance request')
            if parse(source.decode('utf-8')) != g:
                raise ValueError('source changed after initial read')
            if g.get('history_archive') and len(g['history']) <= a.keep:
                if len(source) > DOCTOR_GAP_LIMIT:
                    raise ValueError('gap still exceeds unchanged 5000-byte limit')
                print(f"{g['id']}: history already archived (unchanged)")
                return
            candidate = archived_gap(a.ws, g, source, a.by, a.keep)
            if stable_bytes(path) != source:
                raise ValueError('source changed before commit; original retained')
            save_gap(a.ws, st, candidate)
            print(f"{g['id']}: archived {candidate['history_archive']['events']} events; "
                  f"active gap {len(serialize(candidate).encode())} bytes")
    except (OSError, ValueError, KeyError, TypeError) as e:
        die(f'archive-history refused: {e}')


# ------------------------------------------------------------ index storage
def index_path(ws):
    return os.path.join(gaps_root(ws), "index.json")


def load_index(ws):
    p = index_path(ws)
    if not os.path.exists(p):
        if os.path.exists(os.path.join(ws, "gaps.json")):
            die("found a v1 ledger (gaps.json) — run `ledger.py migrate` once")
        die(f"no ledger at {gaps_root(ws)} — run `ledger.py init` first (or pass --dir / set DEVTEAM_DIR)")
    with open(p) as f:
        return json.load(f)


def summary(g):
    return {k: g.get(k) for k in SUMMARY_KEYS}


@contextlib.contextmanager
def locked(ws):
    idx_p = index_path(ws)
    if not os.path.exists(idx_p):
        load_index(ws)  # dies with a helpful message
    lock = open(os.path.join(ws, ".gaps.lock"), "w")
    try:
        if fcntl:
            fcntl.flock(lock, fcntl.LOCK_EX)
        require_valid_gap_files(ws)
        st = {"index": load_index(ws), "dirty_areas": set()}
        yield st
        atomic_write(idx_p, json.dumps(st["index"], indent=1))
        render_all(ws, st["index"], areas=st["dirty_areas"])
    finally:
        if fcntl:
            fcntl.flock(lock, fcntl.LOCK_UN)
        lock.close()


def find_summary(idx, gid):
    for s in idx["gaps"]:
        if s["id"] == gid:
            return s
    die(f"unknown gap {gid}")


def load_gap(ws, idx, gid):
    s = find_summary(idx, gid)
    p = os.path.join(gaps_root(ws), s["area"], f"{gid}.md")
    try:
        g = parse(stable_bytes(p).decode('utf-8'), p)
        full_history(ws, g)
        return g
    except (OSError, ValueError, KeyError, TypeError) as e:
        die(f"cannot read {p}: {e} — run `ledger.py doctor`")


def save_gap(ws, st, g, old_area=None):
    if g.get("history_archive"):
        full_history(ws, g)
        if len(g['history']) > MAX_HISTORY or len(serialize(g).encode()) > DOCTOR_GAP_LIMIT:
            g = archived_gap(ws, g, serialize(g).encode(), 'ledger', 3)
    atomic_write(gap_path(ws, g), serialize(g))
    idx = st["index"]
    for i, s in enumerate(idx["gaps"]):
        if s["id"] == g["id"]:
            idx["gaps"][i] = summary(g)
            break
    else:
        idx["gaps"].append(summary(g))
    st["dirty_areas"].add(g["area"])
    if old_area and old_area != g["area"]:
        st["dirty_areas"].add(old_area)


# ------------------------------------------------------------------ indexes
def trunc(s, n=INDEX_TITLE):
    s = one_line(s).replace("|", "/")
    return s if len(s) <= n else s[: n - 1] + "…"


def sev_key(s):
    return (SEVERITIES.index(s["severity"]), s["id"])


def render_all(ws, idx, areas=None):
    root = gaps_root(ws)
    gaps = idx["gaps"]
    all_areas = sorted({s["area"] for s in gaps})
    for area in (all_areas if areas is None else sorted(areas)):
        render_area(root, area, [s for s in gaps if s["area"] == area])
    render_top(root, gaps, all_areas)


def row(s, rel_prefix=""):
    owner = s.get("owner") or "-"
    esc = " ⚠ESC" if s["rounds"] >= ESCALATE_AFTER_ROUNDS and s["status"] not in RESOLVED else ""
    return (f"| [{s['id']}]({rel_prefix}{s['id']}.md) | {s['severity']} | {s['status']}{esc} | "
            f"{s.get('task') or '-'} | {owner} | {trunc(s['title'])} |")


def render_area(root, area, items):
    d = os.path.join(root, area)
    os.makedirs(d, exist_ok=True)
    open_items = sorted([s for s in items if s["status"] not in RESOLVED], key=sev_key)
    res = sorted([s for s in items if s["status"] in RESOLVED], key=lambda s: s["id"])
    lines = [f"# Gaps — area `{area}`", "",
             f"Open: {len(open_items)} · resolved: {len(res)} ([RESOLVED.md](RESOLVED.md)) · "
             f"[↑ top index](../INDEX.md)", "",
             "_Auto-generated. Open a gap file only if you act on that gap._", ""]
    if open_items:
        lines += ["| Gap | Sev | Status | Task | Owner | Title |", "|---|---|---|---|---|---|"]
        lines += [row(s) for s in open_items[:AREA_INDEX_ROWS]]
        rest = open_items[AREA_INDEX_ROWS:]
        if rest:
            by = ", ".join(f"{v} {sum(1 for s in rest if s['severity'] == v)}" for v in SEVERITIES
                           if any(s["severity"] == v for s in rest))
            lines += ["", f"_…{len(rest)} more open gaps not listed ({by}). Use "
                      f"`ledger.py list --open --area {area} --owner <id>` or split this area "
                      f"into sub-areas with `ledger.py move`._"]
    else:
        lines.append("_No open gaps._")
    atomic_write(os.path.join(d, "INDEX.md"), "\n".join(lines) + "\n")
    rl = [f"# Resolved gaps — area `{area}`", "", "[← open gaps](INDEX.md)", ""]
    if res:
        rl += ["| Gap | Sev | Outcome | Title |", "|---|---|---|---|"]
        rl += [f"| [{s['id']}]({s['id']}.md) | {s['severity']} | {s['status']} | {trunc(s['title'])} |"
               for s in res]
    else:
        rl.append("_None yet._")
    atomic_write(os.path.join(d, "RESOLVED.md"), "\n".join(rl) + "\n")


def render_top(root, gaps, areas):
    lines = ["# Gap index", "",
             "_Auto-generated by ledger.py. Read this first, then only the area index you work in, "
             "then only the gap files you act on. Never read index.json or whole area folders._", ""]
    open_g = [s for s in gaps if s["status"] not in RESOLVED]
    cnt = {sev: sum(1 for s in open_g if s["severity"] == sev) for sev in SEVERITIES}
    lines.append(f"**Open:** {len(open_g)} (blocker {cnt['blocker']} · major {cnt['major']} · "
                 f"minor {cnt['minor']} · nit {cnt['nit']}) · **Resolved:** {len(gaps) - len(open_g)}")
    lines += ["", "## Areas", ""]
    if areas:
        lines += ["| Area | Open B/M/m/n | Fix to verify | Disputed | Resolved | Index |",
                  "|---|---|---|---|---|---|"]
        for a in areas:
            ag = [s for s in gaps if s["area"] == a]
            ao = [s for s in ag if s["status"] not in RESOLVED]
            bm = "/".join(str(sum(1 for s in ao if s["severity"] == v)) for v in SEVERITIES)
            lines.append(f"| `{a}` | {bm} | {sum(1 for s in ag if s['status'] == 'fix_submitted')} | "
                         f"{sum(1 for s in ag if s['status'] == 'disputed')} | "
                         f"{len(ag) - len(ao)} | [{a}/INDEX.md]({a}/INDEX.md) |")
    else:
        lines.append("_No gaps yet._")
    attention = []
    for title, pred in [
        ("Escalate (≥3 failed rounds)", lambda s: s["rounds"] >= ESCALATE_AFTER_ROUNDS),
        ("Open blockers", lambda s: s["severity"] == "blocker"),
        ("Disputed — need a ruling", lambda s: s["status"] == "disputed"),
        ("Unassigned majors", lambda s: s["severity"] == "major" and s["status"] == "open"),
        ("Fixes awaiting verification", lambda s: s["status"] == "fix_submitted"),
    ]:
        sel = sorted([s for s in open_g if pred(s)], key=sev_key)
        if sel:
            attention.append((title, sel))
    if attention:
        lines += ["", "## Needs attention", ""]
        shown, seen = 0, set()
        for title, sel in attention:
            sel = [s for s in sel if s["id"] not in seen]
            if not sel:
                continue
            lines.append(f"**{title}** ({len(sel)})")
            for s in sel:
                if shown >= ATTENTION_ROWS:
                    break
                seen.add(s["id"])
                shown += 1
                lines.append(f"- [{s['id']}]({s['area']}/{s['id']}.md) {s['severity']}/{s['status']} "
                             f"`{s['area']}` {s.get('owner') or '-'} — {trunc(s['title'], 60)}")
            lines.append("")
            if shown >= ATTENTION_ROWS:
                lines.append(f"_…capped at {ATTENTION_ROWS} rows; see area indexes or "
                             f"`ledger.py list --open --severity blocker,major`._")
                break
    atomic_write(os.path.join(root, "INDEX.md"), "\n".join(lines).rstrip() + "\n")


# ------------------------------------------------------------------ commands
def cmd_init(a):
    if a.max_agents is not None and not 1 <= a.max_agents <= 50:
        die("--max-agents must be between 1 and 50 (hard ceiling)")
    ws = os.path.abspath(a.dir) if a.dir else os.path.join(os.getcwd(), ".devteam")
    for sub in ["tasks", "reports", "evidence", "decisions", "cells", "kit", "gaps"]:
        os.makedirs(os.path.join(ws, sub), exist_ok=True)
    if not os.path.exists(index_path(ws)):
        if os.path.exists(os.path.join(ws, "gaps.json")):
            print("note: v1 gaps.json found — run `ledger.py migrate` to convert it")
        atomic_write(index_path(ws), json.dumps({"version": 2, "next_id": 1, "gaps": []}, indent=1))
        render_all(ws, {"gaps": []})
    skill_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for sub in ["references", "templates", "scripts", "roles"]:
        src = os.path.join(skill_root, sub)
        if os.path.isdir(src):
            dst = os.path.join(ws, "kit", sub)
            shutil.rmtree(dst, ignore_errors=True)
            shutil.copytree(src, dst, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    shutil.copy2(os.path.join(skill_root, "SKILL.md"), os.path.join(ws, "kit", "SKILL.md"))
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import roster
    roster.ensure_files(ws)
    if a.max_agents:
        cfg_p = os.path.join(ws, "config.json")
        cfg = json.load(open(cfg_p))
        cfg["max_concurrent"] = a.max_agents
        json.dump(cfg, open(cfg_p, "w"), indent=2)
    repo = os.path.dirname(ws)
    exclude = os.path.join(repo, ".git", "info", "exclude")
    if os.path.isdir(os.path.dirname(exclude)):
        existing = open(exclude).read() if os.path.exists(exclude) else ""
        if ".devteam/" not in existing:
            with open(exclude, "a") as f:
                f.write("\n.devteam/\n")
    print(f"workspace ready: {ws}")
    print(f"export DEVTEAM_DIR={ws}")
    print(f"gap index: {ws}/gaps/INDEX.md")
    print(f"agent cap: {json.load(open(os.path.join(ws, 'config.json')))['max_concurrent']} concurrent (hard ceiling 50)")


def validate_links(ws, links):
    out = []
    for p in links or []:
        p = p.strip()
        if not p:
            continue
        absp = p if os.path.isabs(p) else os.path.join(os.path.dirname(ws), p)
        if not os.path.exists(absp) and not os.path.exists(os.path.join(ws, p)):
            print(f"ledger: warning: linked path does not exist yet: {p}", file=sys.stderr)
        out.append(p)
    return out


def cmd_add(a):
    check_actor(a.by)
    area = (a.area or "general").lower()
    if not SLUG.match(area):
        die(f"--area '{a.area}' must be a short slug: lowercase letters, digits, - or _ (e.g. checkout, c2-billing)")
    if a.severity not in SEVERITIES:
        die(f"severity must be one of {SEVERITIES}")
    if a.category not in CATEGORIES:
        die(f"category must be one of {CATEGORIES}")
    check_len("title", a.title, MAX_TITLE, "A title states the problem in ≤ 12 words.")
    check_len("desc", a.desc, MAX_DESC, LONG_HINT)
    for name in ("evidence", "expected", "suggest"):
        check_len(name, getattr(a, name), MAX_SHORT, LONG_HINT)
    links = validate_links(a.ws, a.link)
    if len(links) > MAX_LINKS:
        die(f"at most {MAX_LINKS} links per gap — link a folder or an evidence summary file instead")
    if a.severity in ("blocker", "major") and not (a.evidence or links):
        die("blocker/major findings require --evidence (short repro / failing test / reasoning chain) "
            "and/or --link to an evidence file. Downgrade or add evidence.")
    with locked(a.ws) as st:
        idx = st["index"]
        gid = f"G-{idx['next_id']:04d}"
        idx["next_id"] += 1
        g = {"id": gid, "title": one_line(a.title), "severity": a.severity, "category": a.category,
             "status": "open", "area": area, "task": a.task, "location": a.location,
             "phase": a.phase, "raised_by": a.by, "owner": None, "confidence": a.confidence,
             "rounds": 0, "verified_by": None, "created": now(), "updated": now(),
             "description": a.desc, "evidence": a.evidence, "links": links,
             "expected": a.expected, "suggestion": a.suggest, "fix_evidence": None,
             "resolution": None, "history": []}
        log(g, a.by, "add")
        save_gap(a.ws, st, g)
    print(f"{gid}  -> {os.path.relpath(gap_path(a.ws, g), os.path.dirname(a.ws))}")


def transition(a, action):
    check_actor(a.by)
    frm, to = TRANSITIONS[action]
    check_len("note", a.note, MAX_NOTE, LONG_HINT)
    if action in ("dispute", "reopen", "verify") and not a.note:
        die(f"{action} requires --note explaining why (≤ {MAX_NOTE} chars; link longer reasoning)")
    with locked(a.ws) as st:
        g = load_gap(a.ws, st["index"], norm_id(a.id))
        if g["status"] not in frm:
            die(f"{g['id']} is '{g['status']}'; '{action}' needs one of {sorted(frm)}")
        if action == "assign":
            check_actor(a.to, "--to")
            g["owner"] = a.to
        if action == "submit":
            check_len("evidence", a.evidence, MAX_SHORT, LONG_HINT)
            links = validate_links(a.ws, a.link)
            if not (a.evidence or links):
                die("submit requires --evidence and/or --link proving the fix")
            g["fix_evidence"] = a.evidence or ""
            g["links"] = (g["links"] + [l for l in links if l not in g["links"]])[:MAX_LINKS]
        if action == "verify":
            if a.by == g.get("owner"):
                die("the agent who submitted the fix cannot verify it — ask the raiser, QA, or a fresh reviewer")
            g["verified_by"] = a.by
            g["resolution"] = f"verified by {a.by}: {one_line(a.note)}"
        if action == "reopen":
            g["rounds"] += 1
            g["verified_by"] = None
        g["status"] = to
        if action == "reopen" and g.get("owner"):
            g["status"] = "assigned"
        log(g, a.by, action, a.note or (f"to {a.to}" if action == "assign" else ""))
        save_gap(a.ws, st, g)
    warn = f"  [ESCALATE: {g['rounds']} failed rounds]" if (
        g["rounds"] >= ESCALATE_AFTER_ROUNDS and g["status"] not in RESOLVED) else ""
    print(f"{g['id']}: {g['status']}{warn}")


def cmd_rule(a):
    check_actor(a.by)
    if a.decision not in RULE_DECISIONS:
        die(f"decision must be one of {sorted(RULE_DECISIONS)}")
    if not a.note:
        die("rule requires --note with the rationale (link a decisions/D-xxx.md for long ones)")
    check_len("note", a.note, MAX_NOTE, "Put the full rationale in decisions/D-xxx.md and reference it.")
    frm, to = RULE_DECISIONS[a.decision]
    with locked(a.ws) as st:
        g = load_gap(a.ws, st["index"], norm_id(a.id))
        if g["status"] not in frm:
            die(f"{g['id']} is '{g['status']}'; '{a.decision}' needs one of {sorted(frm)}")
        if a.decision in ("wont_fix", "defer") and g["severity"] == "blocker" and not a.human_approved:
            die("a blocker can only be wont_fix/deferred with --human-approved (record who approved in --note)")
        g["status"] = to
        g["resolution"] = f"{a.decision} by {a.by}: {one_line(a.note)}"
        log(g, a.by, f"rule:{a.decision}", a.note)
        save_gap(a.ws, st, g)
    print(f"{g['id']}: {g['status']}")


def cmd_comment(a):
    check_actor(a.by)
    check_len("note", a.note, MAX_NOTE, LONG_HINT)
    with locked(a.ws) as st:
        g = load_gap(a.ws, st["index"], norm_id(a.id))
        log(g, a.by, "comment", a.note)
        save_gap(a.ws, st, g)
    print(f"{g['id']}: comment added")


def cmd_link(a):
    check_actor(a.by)
    links = validate_links(a.ws, a.link)
    if not links:
        die("give at least one --link PATH")
    with locked(a.ws) as st:
        g = load_gap(a.ws, st["index"], norm_id(a.id))
        merged = g["links"] + [l for l in links if l not in g["links"]]
        if len(merged) > MAX_LINKS:
            die(f"at most {MAX_LINKS} links per gap — link an evidence summary file instead")
        g["links"] = merged
        log(g, a.by, "link", ", ".join(links)[:MAX_NOTE])
        save_gap(a.ws, st, g)
    print(f"{g['id']}: {len(g['links'])} link(s)")


def cmd_move(a):
    check_actor(a.by)
    area = a.area.lower()
    if not SLUG.match(area):
        die("--area must be a short lowercase slug")
    with locked(a.ws) as st:
        g = load_gap(a.ws, st["index"], norm_id(a.id))
        old = g["area"]
        if old == area:
            die(f"{g['id']} is already in '{area}'")
        old_path = gap_path(a.ws, g)
        g["area"] = area
        log(g, a.by, "move", f"{old} -> {area}")
        save_gap(a.ws, st, g, old_area=old)
        os.remove(old_path)
    print(f"{g['id']}: moved {old} -> {area}")


def filter_summaries(items, a):
    out = items
    if getattr(a, "open", False):
        out = [s for s in out if s["status"] not in RESOLVED and s["status"] != "deferred"]
    for attr, key in [("status", "status"), ("severity", "severity")]:
        v = getattr(a, attr, None)
        if v:
            want = set(v.split(","))
            out = [s for s in out if s[key] in want]
    for attr in ("area", "owner", "task", "raised_by"):
        v = getattr(a, attr, None)
        if v:
            out = [s for s in out if (s.get(attr) or "") == (v.lower() if attr == "area" else v)]
    return sorted(out, key=sev_key)


def cmd_list(a):
    items = filter_summaries(load_index(a.ws)["gaps"], a)
    total = len(items)
    if a.limit:
        items = items[: a.limit]
    if a.json:
        print(json.dumps(items, indent=1))
        return
    if not items:
        print("(no matching gaps)")
        return
    for s in items:
        esc = " !ESCALATE" if s["rounds"] >= ESCALATE_AFTER_ROUNDS and s["status"] not in RESOLVED else ""
        print(f"{s['id']} {s['severity']:<7} {s['status']:<13} {s['area']:<14} "
              f"task={s.get('task') or '-':<8} owner={s.get('owner') or '-':<12} {trunc(s['title'])}{esc}")
    if a.limit and total > a.limit:
        print(f"… {total - a.limit} more (raise --limit or narrow the filter)")


def cmd_show(a):
    idx = load_index(a.ws)
    g = load_gap(a.ws, idx, norm_id(a.id))
    g['history'] = full_history(a.ws, g)
    g.pop('history_archive', None)
    print(serialize(g), end="")


def cmd_path(a):
    s = find_summary(load_index(a.ws), norm_id(a.id))
    print(os.path.join(gaps_root(a.ws), s["area"], f"{s['id']}.md"))


def cmd_gate(a):
    items = load_index(a.ws)["gaps"]
    if a.area:
        items = [s for s in items if s["area"] == a.area.lower()]
    problems = []
    for s in items:
        st, sev = s["status"], s["severity"]
        if st in RESOLVED:
            continue
        if a.phase == "design":
            if s.get("phase") == "design" and sev in ("blocker", "major") and st != "deferred":
                problems.append(s)
        elif sev == "blocker" or (sev == "major" and st != "deferred") or st == "fix_submitted":
            problems.append(s)
        elif a.phase == "done" and sev == "minor" and st != "deferred":
            problems.append(s)
    scope = f" [{a.area}]" if a.area else ""
    if problems:
        print(f"GATE '{a.phase}'{scope}: FAIL — {len(problems)} item(s) must be resolved:")
        for s in sorted(problems, key=sev_key)[:40]:
            print(f"  {s['id']} [{s['severity']}/{s['status']}] {s['area']} {trunc(s['title'])} (owner={s.get('owner')})")
        if len(problems) > 40:
            print(f"  … {len(problems) - 40} more (`ledger.py list --open`)")
        sys.exit(1)
    print(f"GATE '{a.phase}'{scope}: PASS")


def cmd_stats(a):
    items = load_index(a.ws)["gaps"]
    if a.area:
        items = [s for s in items if s["area"] == a.area.lower()]
    print(f"total gaps: {len(items)} in {len({s['area'] for s in items})} area(s)")
    for st in STATUSES:
        sel = [s for s in items if s["status"] == st]
        if sel:
            parts = ", ".join(f"{v}={sum(1 for s in sel if s['severity'] == v)}"
                              for v in SEVERITIES if any(s["severity"] == v for s in sel))
            print(f"  {st:<13} {parts}")
    esc = [s["id"] for s in items if s["rounds"] >= ESCALATE_AFTER_ROUNDS and s["status"] not in RESOLVED]
    if esc:
        print(f"  needs escalation: {', '.join(esc)}")


def cmd_render(a):
    with locked(a.ws) as st:
        st['dirty_areas'].update(s['area'] for s in st['index']['gaps'])
    print(os.path.join(gaps_root(a.ws), "INDEX.md"))


def scan_gap_files(ws):
    root = gaps_root(ws)
    found, errors = [], []
    if not os.path.isdir(root):
        return found, errors
    for area in sorted(os.listdir(root)):
        d = os.path.join(root, area)
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d)):
            if re.match(r"^G-\d{4,}\.md$", f):
                p = os.path.join(d, f)
                try:
                    g = parse(stable_bytes(p).decode('utf-8'), p)
                    full_history(ws, g)
                    if g["area"] != area or f"{g['id']}.md" != f:
                        errors.append(f"{p}: id/area in file don't match its location")
                    found.append(g)
                except Exception as e:  # noqa: BLE001
                    errors.append(f"{p}: {e}")
    return found, errors


def cmd_reindex(a):
    lock = open(os.path.join(a.ws, ".gaps.lock"), "w")
    try:
        if fcntl:
            fcntl.flock(lock, fcntl.LOCK_EX)
        found = require_valid_gap_files(a.ws)
        nxt = max([int(g["id"][2:]) for g in found] + [0]) + 1
        old = load_index(a.ws) if os.path.exists(index_path(a.ws)) else {}
        idx = {"version": 2, "next_id": max(nxt, old.get("next_id", 1)), "gaps": [summary(g) for g in found]}
        atomic_write(index_path(a.ws), json.dumps(idx, indent=1))
        render_all(a.ws, idx)
        print(f"reindexed {len(found)} gap(s); next id G-{idx['next_id']:04d}")
    finally:
        if fcntl:
            fcntl.flock(lock, fcntl.LOCK_UN)
        lock.close()


def require_valid_gap_files(ws):
    found, errors = scan_gap_files(ws)
    if errors:
        die('refusing ledger mutation/index refresh: ' + '; '.join(errors))
    return found


def cmd_doctor(a):
    ws = a.ws
    repo = os.path.dirname(ws)
    problems = []
    found, errors = scan_gap_files(ws)
    problems += [f"broken gap file: {e}" for e in errors]
    if os.path.exists(index_path(ws)):
        ids_idx = {s["id"] for s in load_index(ws)["gaps"]}
        ids_files = {g["id"] for g in found}
        if ids_idx != ids_files:
            problems.append(f"index.json out of sync with gap files "
                            f"(missing files: {sorted(ids_idx - ids_files)[:5]}, "
                            f"unindexed files: {sorted(ids_files - ids_idx)[:5]}) — run `ledger.py reindex`")
    open_by_area = {}
    for g in found:
        if g["status"] not in RESOLVED:
            open_by_area[g["area"]] = open_by_area.get(g["area"], 0) + 1
    for area, n in sorted(open_by_area.items()):
        if n > AREA_INDEX_ROWS:
            problems.append(f"area '{area}' has {n} open gaps (> {AREA_INDEX_ROWS}) — split it into "
                            f"sub-areas (e.g. {area}-auth, {area}-api) with `ledger.py move`")
    for g in found:
        p = gap_path(ws, g)
        if os.path.getsize(p) > DOCTOR_GAP_LIMIT:
            problems.append(f"oversized gap file ({os.path.getsize(p)} B): {p} — move content to evidence/ and --link it")
    skip = {".git", "node_modules", ".venv", "venv", "dist", "build", "kit", "evidence", "__pycache__"}
    stray = re.compile(r"(^|[-_.])(gaps?|findings|issues[-_]found|review[-_]notes)([-_.].*)?\.md$", re.I)
    for base, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in skip and (not d.startswith(".") or d == ".devteam")]
        rel = os.path.relpath(base, repo)
        depth = 0 if rel == "." else rel.count(os.sep) + 1
        if depth > 6:
            dirs[:] = []
            continue
        in_gaps = os.path.abspath(base).startswith(gaps_root(ws))
        for f in files:
            p = os.path.join(base, f)
            if not in_gaps and stray.search(f):
                problems.append(f"stray findings file (use `ledger.py add`, not ad-hoc files): {os.path.relpath(p, repo)}")
            if os.path.abspath(p).startswith(ws) and f.endswith(".md") and not in_gaps:
                if os.path.getsize(p) > DOCTOR_MD_LIMIT:
                    problems.append(f"oversized workspace file ({os.path.getsize(p)} B): {os.path.relpath(p, repo)} "
                                    f"— split it into an index + linked files")
    if problems:
        print(f"doctor: {len(problems)} problem(s)")
        for p in problems:
            print(f"  - {p}")
        sys.exit(1)
    print(f"doctor: OK ({len(found)} gap files)")


def cmd_migrate(a):
    v1 = os.path.join(a.ws, "gaps.json")
    if not os.path.exists(v1):
        die("no v1 gaps.json to migrate")
    data = json.load(open(v1))
    os.makedirs(gaps_root(a.ws), exist_ok=True)
    n = 0
    for old in data.get("gaps", []):
        gid = norm_id(old["id"])
        area = "general"
        if old.get("task"):
            m = re.match(r"^T-([A-Za-z0-9]+)-", old["task"])
            area = m.group(1).lower() if m else "general"
        long_parts = []
        desc = old.get("description") or ""
        if len(desc) > MAX_DESC:
            long_parts.append(("description", desc))
            desc = desc[:MAX_DESC - 80] + " … (full text in linked migrated file)"
        ev = old.get("evidence") or ""
        if len(ev) > MAX_SHORT:
            long_parts.append(("evidence", ev))
            ev = ev[:MAX_SHORT - 60] + " … (full text linked)"
        links = []
        if long_parts:
            ep = os.path.join(a.ws, "evidence", "migrated", f"{gid}.md")
            atomic_write(ep, "\n\n".join(f"## {k}\n{v}" for k, v in long_parts) + "\n")
            links.append(os.path.relpath(ep, os.path.dirname(a.ws)))
        g = {"id": gid, "title": one_line(old.get("title", ""))[:MAX_TITLE], "severity": old["severity"],
             "category": old["category"], "status": old["status"], "area": area, "task": old.get("task"),
             "location": old.get("location"), "phase": old.get("phase"), "raised_by": old.get("raised_by"),
             "owner": old.get("owner"), "confidence": old.get("confidence"), "rounds": old.get("rounds", 0),
             "verified_by": old.get("verified_by"), "created": old.get("created"), "updated": old.get("updated"),
             "description": desc, "evidence": ev, "links": links, "expected": old.get("expected"),
             "suggestion": old.get("suggestion"), "fix_evidence": old.get("fix_evidence"),
             "resolution": old.get("resolution"),
             "history": [{"ts": h["ts"], "by": h["by"], "action": h["action"],
                          "note": one_line(h.get("note", ""))[:MAX_NOTE]} for h in old.get("history", [])][-MAX_HISTORY:]}
        atomic_write(gap_path(a.ws, g), serialize(g))
        n += 1
    os.replace(v1, v1 + ".v1-migrated")
    a2 = argparse.Namespace(ws=a.ws)
    cmd_reindex(a2)
    print(f"migrated {n} gap(s); old file kept as gaps.json.v1-migrated")


# ---------------------------------------------------------------------- cli
def main():
    p = argparse.ArgumentParser(description="dev-team gap ledger (indexed, one file per gap)")
    p.add_argument("--dir", help="path to the .devteam workspace")
    sp = p.add_subparsers(dest="cmd", required=True)

    s = sp.add_parser("init")
    s.add_argument("--max-agents", type=int, help="global concurrent-agent cap (1-50, default 50)")

    s = sp.add_parser("add")
    s.add_argument("--by", required=True)
    s.add_argument("--area", help="feature/module/cell slug the gap belongs to (default: general)")
    s.add_argument("--title", required=True)
    s.add_argument("--severity", required=True)
    s.add_argument("--category", required=True)
    s.add_argument("--desc", required=True)
    s.add_argument("--task")
    s.add_argument("--location", help="file:line, endpoint, screen, AC id...")
    s.add_argument("--phase", default="review", choices=["design", "implementation", "review", "integration", "final"])
    s.add_argument("--evidence", help=f"short inline repro (≤{MAX_SHORT} chars); link files for more")
    s.add_argument("--link", action="append", help="path to an evidence file (repeatable)")
    s.add_argument("--expected")
    s.add_argument("--suggest")
    s.add_argument("--confidence", choices=["high", "medium", "low"], default="medium")

    for name in ["assign", "submit", "verify", "reopen", "dispute"]:
        s = sp.add_parser(name)
        s.add_argument("id")
        s.add_argument("--by", required=True)
        s.add_argument("--note")
        if name == "assign":
            s.add_argument("--to", required=True)
        if name == "submit":
            s.add_argument("--evidence")
            s.add_argument("--link", action="append")

    s = sp.add_parser("rule")
    s.add_argument("id")
    s.add_argument("--by", required=True)
    s.add_argument("--decision", required=True)
    s.add_argument("--note")
    s.add_argument("--human-approved", action="store_true")

    s = sp.add_parser("comment")
    s.add_argument("id")
    s.add_argument("--by", required=True)
    s.add_argument("--note", required=True)

    s = sp.add_parser("link")
    s.add_argument("id")
    s.add_argument("--by", required=True)
    s.add_argument("--link", action="append", required=True)

    s = sp.add_parser("move")
    s.add_argument("id")
    s.add_argument("--by", required=True)
    s.add_argument("--area", required=True)

    s = sp.add_parser('archive-history', help='losslessly archive history under a source hash guard')
    s.add_argument('id')
    s.add_argument('--by', required=True)
    s.add_argument('--keep', type=int, default=3)
    s.add_argument('--expected-sha256', required=True)

    s = sp.add_parser("list")
    s.add_argument("--open", action="store_true")
    s.add_argument("--area")
    s.add_argument("--status")
    s.add_argument("--severity")
    s.add_argument("--owner")
    s.add_argument("--task")
    s.add_argument("--raised-by")
    s.add_argument("--limit", type=int)
    s.add_argument("--json", action="store_true")

    for name in ("show", "path"):
        s = sp.add_parser(name)
        s.add_argument("id")

    s = sp.add_parser("gate")
    s.add_argument("--phase", default="review", choices=["design", "review", "done"])
    s.add_argument("--area")

    s = sp.add_parser("stats")
    s.add_argument("--area")

    for name in ("render", "reindex", "doctor", "migrate"):
        sp.add_parser(name)

    a = p.parse_args()
    if a.cmd == "init":
        return cmd_init(a)
    a.ws = find_dir(a.dir)
    if a.cmd in TRANSITIONS:
        return transition(a, a.cmd)
    return {"add": cmd_add, "rule": cmd_rule, "comment": cmd_comment, "link": cmd_link,
            "move": cmd_move, "list": cmd_list, "show": cmd_show, "path": cmd_path,
            "gate": cmd_gate, "stats": cmd_stats, "render": cmd_render, "reindex": cmd_reindex,
            "doctor": cmd_doctor, "migrate": cmd_migrate,
            "archive-history": cmd_archive_history}[a.cmd](a)


if __name__ == "__main__":
    main()
