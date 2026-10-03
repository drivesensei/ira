"""T049 child-only executor. Trusted callers pin receipts; no production target grant.

Environment namespaces are not OS confinement. All runs are retained and marked
INCOMPLETE because waitpid cannot establish escaped-descendant completion.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import subprocess
import time


class Rejected(ValueError):
    pass


def digest(path):
    with open(path, 'rb') as stream:
        hasher = hashlib.sha256()
        while block := stream.read(1024 * 1024):
            hasher.update(block)
        return hasher.hexdigest()


def canonical(path):
    p = Path(path)
    if not p.is_absolute() or str(p.resolve(strict=True)) != str(p):
        raise Rejected('path must be absolute canonical and non-symlink')
    for component in (p, *p.parents):
        if component.is_symlink():
            raise Rejected('symlink path')
    return p


def identity(path):
    s = path.stat()
    return dict(path=str(path), uid=s.st_uid, mode=oct(stat.S_IMODE(s.st_mode)),
                device=s.st_dev, inode=s.st_ino, length=len(str(path)))


def owned_directory(path):
    p = canonical(path)
    s = p.stat()
    if not stat.S_ISDIR(s.st_mode) or s.st_uid != os.getuid() or stat.S_IMODE(s.st_mode) != 0o700:
        raise Rejected('directory must be owned 0700')
    if not re.fullmatch(r'/[A-Za-z0-9_./-]+', str(p)):
        raise Rejected('namespace must have short-script-safe ASCII path')
    return p


def verify(receipt, pinned_digest, allowed):
    """allowed maps target -> exact trusted receipt digest (external trust anchor)."""
    encoded = json.dumps(receipt, sort_keys=True, separators=(',', ':')).encode()
    actual = hashlib.sha256(encoded).hexdigest()
    if actual != pinned_digest or allowed.get(receipt.get('target')) != actual:
        raise Rejected('receipt/target not pinned')
    if receipt.get('kind') != 'synthetic' or not receipt['target'].startswith('synthetic-'):
        raise Rejected('real targets remain disabled pending independent clearance')
    if set(receipt) != {'target', 'kind', 'source_revision', 'build_argv', 'argv', 'artifacts'}:
        raise Rejected('unknown or missing receipt fields')
    if not receipt['source_revision'] or not receipt['build_argv']:
        raise Rejected('missing source/build provenance')
    artifacts = receipt['artifacts']
    if set(artifacts) != {'binary', 'compiler', 'sources'} or not artifacts['sources']:
        raise Rejected('missing artifact provenance')
    all_artifacts = [artifacts['binary'], artifacts['compiler'], *artifacts['sources']]
    for artifact in all_artifacts:
        if set(artifact) != {'path', 'sha256'}:
            raise Rejected('unknown artifact field')
        p = canonical(artifact['path'])
        if not p.is_file() or digest(p) != artifact['sha256']:
            raise Rejected('artifact hash/type mismatch')
    argv = receipt['argv']
    if not isinstance(argv, list) or not argv or any(not isinstance(a, str) or '\x00' in a for a in argv):
        raise Rejected('invalid exact argv')
    if argv[0] != artifacts['binary']['path']:
        raise Rejected('binary/argv mismatch')
    return actual


def run(receipt, pinned_digest, allowed, parent, name, *, timeout=5, overrides=None):
    """No shell, no inherited environment, no PATH, no kill or cleanup policy.

    parent is a trusted task-owned canonical 0700 directory. Caller must retain it.
    Only strict locale/timezone enums may be set; all executable overrides denied.
    """
    authentication = verify(receipt, pinned_digest, allowed)
    parent = owned_directory(parent)
    if parent.parent != Path('/private/tmp') or not parent.name.startswith('t049-'):
        raise Rejected('parent must be a dedicated canonical T049 tmp root')
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,32}', name):
        raise Rejected('invalid exclusive run name')
    if not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or timeout <= 0 or timeout > 60:
        raise Rejected('invalid timeout')
    supplied = overrides or {}
    enums = {'LANG': {'C'}, 'LC_ALL': {'C'}, 'TZ': {'UTC'}}
    if any(k not in enums or v not in enums[k] for k, v in supplied.items()):
        raise Rejected('unsafe environment override')
    root = parent / name
    try:
        root.mkdir(mode=0o700)
    except FileExistsError as exc:
        raise Rejected('namespace already exists; never reuse') from exc
    # Nothing below this line is ever deleted, even on failure.
    state = {'schema': 1, 'authentication': authentication, 'build': receipt,
             'status': 'INCOMPLETE', 'reason': 'not launched', 'retained': True,
             'root': identity(root), 'descendants': 'UNVERIFIED'}
    def save():
        data = json.dumps(state, indent=2, sort_keys=True).encode()
        fd = os.open(root / 'receipt.json', os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
    save()
    try:
        owned_directory(root)
        children = {}
        for label in ('home', 'tmp', 'cache', 'cwd'):
            p = root / label
            p.mkdir(mode=0o700)
            children[label] = owned_directory(p)
        state['directories'] = {k: identity(v) for k, v in children.items()}
        env = {'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC', **supplied,
               'HOME': str(children['home']), 'TMPDIR': str(children['tmp']),
               'IRA_THUMBNAIL_CACHE_DIR': str(children['cache']), 'PWD': str(children['cwd'])}
        # Snapshot executable bytes into exclusively-owned namespace to prevent
        # mutation of the external binary between authentication and launch.
        original = canonical(receipt['artifacts']['binary']['path'])
        executable = root / 'binary'
        with original.open('rb') as src, executable.open('xb') as dst:
            while block := src.read(1024 * 1024):
                dst.write(block)
        executable.chmod(0o500)
        if digest(executable) != receipt['artifacts']['binary']['sha256']:
            raise Rejected('binary changed during snapshot')
        verify(receipt, pinned_digest, allowed)
        mapped = {}
        for index, artifact in enumerate(receipt['artifacts']['sources']):
            snapshot = root / ('source-' + str(index))
            with canonical(artifact['path']).open('rb') as src, snapshot.open('xb') as dst:
                dst.write(src.read())
            snapshot.chmod(0o400)
            if digest(snapshot) != artifact['sha256']:
                raise Rejected('source changed during snapshot')
            mapped[artifact['path']] = str(snapshot)
        argv = [str(executable), *[mapped.get(arg, arg) for arg in receipt['argv'][1:]]]
        state.update(argv=argv, authenticated_argv=receipt['argv'], environment=env,
                     executable=identity(executable), launch_time_ns=time.time_ns())
        with (root / 'stdout').open('xb') as out, (root / 'stderr').open('xb') as err:
            process = subprocess.Popen(argv, cwd=children['cwd'], env=env,
                                       stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                       start_new_session=True, close_fds=True)
            state.update(pid=process.pid, pgid=process.pid, session=process.pid,
                         start_identity='Popen-owned direct child; launch_time_ns recorded',
                         reason='direct child running; descendant completion unverified')
            save()
            try:
                state['exit_code'] = process.wait(timeout=timeout)
                state['reason'] = 'direct child exited; descendant completion unverified'
            except subprocess.TimeoutExpired:
                state.update(exit_code=None, reason='timeout; no stop authorized; child may remain alive')
            state['stdout_sha256'] = digest(root / 'stdout')
            state['stderr_sha256'] = digest(root / 'stderr')
            state['output_snapshot_only'] = True
            state['test_summary_counts'] = 'UNVERIFIED; raw output retained, no libtest grant'
    except Exception as exc:
        state['reason'] = 'launcher failure: ' + type(exc).__name__
        save()
        raise
    save()
    return state
