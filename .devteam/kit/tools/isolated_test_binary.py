"""T049 trusted synthetic executor, not OS confinement. Real targets disabled.
All runs retained/INCOMPLETE: escaped descendant completion is unverified.
"""
from contextlib import ExitStack
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


def stat_key(info):
    return (info.st_dev, info.st_ino, info.st_uid, info.st_mode)


def digest_fd(fd):
    """Read a bounded byte snapshot without changing the child's output offset."""
    hasher = hashlib.sha256()
    size, offset = os.fstat(fd).st_size, 0
    while offset < size:
        block = os.pread(fd, min(1024 * 1024, size - offset), offset)
        if not block:
            break
        hasher.update(block)
        offset += len(block)
    return hasher.hexdigest()


class Authority:
    """Held handles and lstat checks. Same-UID byte edits are not prevented."""
    def __init__(self, stack, parent, name):
        self.stack, self.parent, self.name = stack, parent, name
        self.entries, self.failures = {}, []
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
        self.parent_fd = self.keep(os.open(parent, flags))
        parent_info = os.fstat(self.parent_fd)
        if parent_info.st_uid != os.getuid() or stat.S_IMODE(parent_info.st_mode) != 0o700:
            raise Rejected('parent FD must be owned 0700')
        self.parent_key = stat_key(parent_info)
        if stat_key(os.stat(parent, follow_symlinks=False)) != self.parent_key:
            raise Rejected('parent identity changed')
        try:
            os.mkdir(name, mode=0o700, dir_fd=self.parent_fd)
        except FileExistsError as exc:
            raise Rejected('namespace already exists; never reuse') from exc
        self.root_fd = self.keep(os.open(name, flags, dir_fd=self.parent_fd))
        root_info = os.fstat(self.root_fd)
        if root_info.st_uid != os.getuid() or stat.S_IMODE(root_info.st_mode) != 0o700:
            raise Rejected('root FD must be owned 0700')
        self.root_key = stat_key(root_info)
        self.receipt_fd = self.create('receipt.json')

    def keep(self, fd):
        self.stack.callback(os.close, fd)
        return fd

    def create(self, name, mode=0o600):
        fd = self.keep(os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                               mode, dir_fd=self.root_fd))
        self.entries[name] = (fd, stat_key(os.fstat(fd)))
        return fd

    def directory(self, name):
        os.mkdir(name, mode=0o700, dir_fd=self.root_fd)
        fd = self.keep(os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                               dir_fd=self.root_fd))
        self.entries[name] = (fd, stat_key(os.fstat(fd)))
        return fd

    def refresh_mode(self, name, mode):
        fd, _ = self.entries[name]
        os.fchmod(fd, mode)
        self.entries[name] = (fd, stat_key(os.fstat(fd)))

    def errors(self):
        failures = []
        references = [('parent', None, self.parent, self.parent_fd, self.parent_key),
                      ('root', self.parent_fd, self.name, self.root_fd, self.root_key)]
        references.extend((name, self.root_fd, name, fd, key)
                          for name, (fd, key) in self.entries.items())
        for label, directory_fd, name, fd, key in references:
            try:
                current = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                held = os.fstat(fd)
                if stat_key(current) != key or stat_key(held) != key:
                    failures.append(label + ': identity or mode changed')
                elif stat.S_ISREG(held.st_mode) and held.st_nlink != 1:
                    failures.append(label + ': regular file link count changed')
            except OSError as exc:
                failures.append(label + ': ' + type(exc).__name__)
        self.failures = list(dict.fromkeys([*self.failures, *failures]))
        return self.failures

    def save(self, state):
        errors = self.errors()
        state['namespace_integrity'] = 'COMPROMISED' if errors else 'INTACT_SNAPSHOT'
        state['authority_errors'] = errors
        data = json.dumps(state, indent=2, sort_keys=True).encode()
        # Use held inode even if child renamed/replaced the receipt.
        offset = 0
        while offset < len(data):
            written = os.pwrite(self.receipt_fd, data[offset:], offset)
            if written <= 0:
                raise OSError('receipt write made no progress')
            offset += written
        os.ftruncate(self.receipt_fd, len(data))
        os.fsync(self.receipt_fd)
        return errors


def run(receipt, pinned_digest, allowed, parent, name, *, timeout=5, overrides=None):
    """Child-only explicit environment in a trusted retained task tmp parent."""
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
    # Nothing created below is deleted, even after failure or replacement.
    with ExitStack() as descriptors:
        authority = Authority(descriptors, parent, name)
        state = {'schema': 1, 'authentication': authentication, 'build': receipt,
                 'status': 'INCOMPLETE', 'reason': 'not launched', 'retained': True,
                 'root': identity(root), 'descendants': 'UNVERIFIED'}
        authority.save(state)
        try:
            children = {}
            for label in ('home', 'tmp', 'cache', 'cwd'):
                authority.directory(label)
                children[label] = root / label
            state['directories'] = {k: identity(v) for k, v in children.items()}
            env = {'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC', **supplied,
                   'HOME': str(children['home']), 'TMPDIR': str(children['tmp']),
                   'IRA_THUMBNAIL_CACHE_DIR': str(children['cache']), 'PWD': str(children['cwd'])}
            original = canonical(receipt['artifacts']['binary']['path'])
            executable = root / 'binary'
            binary_fd = authority.create('binary')
            with original.open('rb') as src, os.fdopen(os.dup(binary_fd), 'wb') as dst:
                while block := src.read(1024 * 1024):
                    dst.write(block)
            authority.refresh_mode('binary', 0o500)
            if digest_fd(binary_fd) != receipt['artifacts']['binary']['sha256']:
                raise Rejected('binary changed during snapshot')
            verify(receipt, pinned_digest, allowed)
            mapped = {}
            for index, artifact in enumerate(receipt['artifacts']['sources']):
                label = 'source-' + str(index)
                snapshot = root / label
                fd = authority.create(label)
                with canonical(artifact['path']).open('rb') as src, os.fdopen(os.dup(fd), 'wb') as dst:
                    while block := src.read(1024 * 1024):
                        dst.write(block)
                authority.refresh_mode(label, 0o400)
                if digest_fd(fd) != artifact['sha256']:
                    raise Rejected('source changed during snapshot')
                mapped[artifact['path']] = str(snapshot)
            argv = [str(executable), *[mapped.get(arg, arg) for arg in receipt['argv'][1:]]]
            state.update(argv=argv, authenticated_argv=receipt['argv'], environment=env,
                         executable=identity(executable), launch_time_ns=time.time_ns())
            out_fd, err_fd = authority.create('stdout'), authority.create('stderr')
            state['evidence_handles'] = {label: {'device': os.fstat(fd).st_dev,
                                                'inode': os.fstat(fd).st_ino}
                                        for label, fd in (('receipt', authority.receipt_fd),
                                                          ('stdout', out_fd), ('stderr', err_fd))}
            if authority.save(state):
                raise Rejected('namespace authority changed before launch')
            process = subprocess.Popen(argv, cwd=children['cwd'], env=env,
                                       stdin=subprocess.DEVNULL, stdout=out_fd, stderr=err_fd,
                                       start_new_session=True, close_fds=True)
            state.update(pid=process.pid, pgid=process.pid, session=process.pid,
                         start_identity='Popen-owned child; launch_time_ns recorded',
                         reason='direct child running; descendant completion unverified')
            authority.save(state)
            try:
                state['exit_code'] = process.wait(timeout=timeout)
                state['reason'] = 'direct child exited; descendant completion unverified'
            except subprocess.TimeoutExpired:
                state.update(exit_code=None, reason='timeout; no stop authorized; child may remain alive')
            # No pathname is reopened for reading or writing after child launch.
            state['stdout_sha256'] = digest_fd(out_fd)
            state['stderr_sha256'] = digest_fd(err_fd)
            state['output_snapshot_only'] = True
            state['test_summary_counts'] = 'UNVERIFIED; raw output retained, no libtest grant'
            if authority.save(state):
                raise Rejected('child replaced namespace/evidence authority')
        except Exception as exc:
            state['reason'] = 'launcher failure: ' + type(exc).__name__
            authority.save(state)
            raise
        return state
