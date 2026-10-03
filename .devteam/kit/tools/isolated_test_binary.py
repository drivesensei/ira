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


# T052 gates1/2 only. These constants identify source, not an executable grant.
_PUBLICATION_COMMIT = 'e157ad2d8c3d5ea5a6a70df0a032be33cf4a3868'
_PUBLICATION_TREE = 'f80d8f0cf6015be1cf554ed6021e2e34e6b02ec5'
_PUBLICATION_TEST = 'legacy_save_publishes_only_complete_old_or_new_snapshots'
# Tuple values cannot be enabled by a caller mutating a returned policy copy.
_PUBLICATION_SOURCES = {
    'e157-core-publication-macos': (
        'ira-core', '0.1.0', 'crates/core/Cargo.toml',
        '7dd8727ef92b77005c87b57264bef0ab8650466c',
        '56f2057943be9c943f6c00caaef5ec3fd935bef1787e79cbbb61e3fc5c8c9f80',
        '1a523d3cf0f33cea633b4a61c4e9832f9821fc9f',
        '44b8e7e90c36f1e0c5a80ea8491775f803650b82d2a1df0e0c5edbeddd671864',
        '89d0fd022e6314426aafd6f8c51ca95fcc6401e2',
        'f97a88a72de9d4f42c42342650bda3fe1d799ae4958c4c0900c619f76fce9fac'),
    'e157-root-publication-macos': (
        'ira', '0.1.21', 'Cargo.toml',
        '77b091014b5341b022eabe515ae12e7828c06b2a',
        '7152504059428bb1fccbcc14a3c96cc0d3dae138edb0e3dd9c0e3538f610d8e2',
        'ae541564e0a843254804bb58c9807df3f0293824',
        'b9fd04af13159525f1f98b96935b0ec499d2f1072369aae5e133dc28fdb166bc',
        'ad79b66ca5f5deed8c5a8883fa510a65adbd9871',
        '971ea5572a01cc723b02ab18dbb6db04a28e8ea84ba762fae5f351bbe1d0f28d'),
}
_PUBLICATION_ENV = {'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC',
                    'HOME': '$OWNED_HOME', 'TMPDIR': '$OWNED_TMP',
                    'IRA_THUMBNAIL_CACHE_DIR': '$OWNED_CACHE', 'PWD': '$OWNED_CWD'}
_PUBLICATION_DIGESTS = (
    'source_receipt_sha256', 'source_input_manifest_sha256',
    'source_before_manifest_sha256', 'source_after_manifest_sha256',
    'build_receipt_sha256', 'build_action_manifest_sha256',
    'capture_review_sha256', 'actual_invocations_sha256', 'compiler_sha256',
    'std_libtest_manifest_sha256', 'dependencies_manifest_sha256',
    'sdk_link_manifest_sha256', 'library_compile_attestation_sha256',
    'test_compile_attestation_sha256', 'startup_attestation_sha256',
    'signature_attestation_sha256', 'runtime_boundary_review_sha256',
    'binary_sha256', 'executor_source_sha256', 'fd_review_sha256',
    'coordinator_scope_review_sha256')


def publication_policy(entry_id):
    """Return a detached disabled source recipe; not an authority capability."""
    if type(entry_id) is not str or entry_id not in _PUBLICATION_SOURCES:
        raise Rejected('unknown fixed publication entry')
    names = ('package', 'version', 'manifest', 'manifest_blob', 'manifest_sha256',
             'lock_blob', 'lock_sha256', 'test_blob', 'test_sha256')
    policy = dict(zip(names, _PUBLICATION_SOURCES[entry_id]))
    core = entry_id == 'e157-core-publication-macos'
    prefix = 'crates/core/' if core else ''
    policy.update(lock_source=prefix + 'Cargo.lock',
                  test_source=prefix + 'tests/persistence_publication_regression.rs',
                  library_source=prefix + 'src/lib.rs',
                  library_blob=('4c2dbc1925ed6fc3ed654cb1800ec3afe01dd6d0' if core else
                                '376ba6674ebce1c23d597b4f0694cb5aa087e80f'),
                  library_sha256=('6e93ca97a8df88c78549ad9b9cf49842c1a93d89faca640e4b3ecabc855fcb81' if core else
                                  'c2530e2cb4a88fef1715e9185c658b1359e71439c3e79c49b4ea68833e92fba3'),
                  enabled=False, entry_id=entry_id, source_commit=_PUBLICATION_COMMIT,
                  source_tree=_PUBLICATION_TREE, target='persistence_publication_regression',
                  inventory=[_PUBLICATION_TEST], host='aarch64-apple-darwin',
                  target_triple='aarch64-apple-darwin', profile='test', features=[],
                  default_features=True, cfg_recipe='darwin-debug-ordinary-library-plus-harness',
                  recipe='offline-locked-debug-integration-v1', library_cfg_test=False,
                  harness_cfg_test=True, artifact_count=1,
                  argv=['$OWNED_BINARY'], environment=dict(_PUBLICATION_ENV))
    return policy


def check_publication_approval_schema(entry_id, canonical_bytes):
    """Shape/consistency ONLY, never authentication, artifact I/O or admission.

    The trusted-parent channel, actual gate3/4/5 attestations, run consumption and
    clock/expiry enforcement do not exist in this tranche. Same-UID JSON hashes
    are not authority. Even a complete record cannot enable either real entry.
    """
    policy = publication_policy(entry_id)
    expected = {k: v for k, v in policy.items() if k != 'enabled'}
    expected.update(schema_version=1, approval_status='approved',
                    fresh_library_and_test_compilation=True,
                    expected_counts={'passed': 1, 'failed': 0, 'ignored': 0,
                                     'measured': 0, 'filtered_out': 0},
                    observation='retained-output-snapshot-descendants-incomplete')
    extra = {'executor_commit', 'run_id', 'issued_at_epoch', 'expires_at_epoch',
             'timeout_seconds', *_PUBLICATION_DIGESTS}
    if type(canonical_bytes) is not bytes or not 0 < len(canonical_bytes) <= 65536:
        raise Rejected('approval must be bounded canonical bytes')
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise Rejected('duplicate approval key')
            result[key] = value
        return result
    try:
        record = json.loads(canonical_bytes.decode('ascii'), object_pairs_hook=unique_object)
        normalized = json.dumps(record, sort_keys=True, separators=(',', ':'),
                                ensure_ascii=True, allow_nan=False).encode('ascii')
    except (UnicodeError, ValueError, TypeError, RecursionError) as exc:
        raise Rejected('invalid canonical approval encoding') from exc
    if normalized != canonical_bytes or type(record) is not dict:
        raise Rejected('noncanonical approval encoding')
    if set(record) != set(expected) | extra:
        raise Rejected('unknown or missing approval field')
    for key, value in expected.items():
        # bool/int equivalence must not admit cfg/schema/count type swaps.
        if type(record[key]) is not type(value) or record[key] != value:
            raise Rejected('approval disagrees with fixed publication recipe')
        if key == 'expected_counts' and any(type(n) is not int for n in record[key].values()):
            raise Rejected('invalid result count type')
    for key in _PUBLICATION_DIGESTS:
        value = record[key]
        if type(value) is not str or not re.fullmatch('[0-9a-f]{64}', value) or value == '0' * 64:
            raise Rejected('missing complete attestation digest')
    if not record['source_input_manifest_sha256'] == record['source_before_manifest_sha256'] == record['source_after_manifest_sha256']:
        raise Rejected('source input bytes changed across build')
    for key, length in (('executor_commit', 40), ('run_id', 32)):
        value = record[key]
        if type(value) is not str or not re.fullmatch('[0-9a-f]{' + str(length) + '}', value) or value == '0' * length:
            raise Rejected('invalid executor/run identity')
    for key in ('issued_at_epoch', 'expires_at_epoch', 'timeout_seconds'):
        if type(record[key]) is not int:
            raise Rejected('invalid observation/expiry type')
    if not 0 < record['issued_at_epoch'] < record['expires_at_epoch'] < 2 ** 63:
        raise Rejected('invalid grant validity interval')
    if not 0 < record['timeout_seconds'] <= 60:
        raise Rejected('invalid approved observation timeout')
    # Deliberately returns no authenticated record, launch object or capability.


def _check_publication_count_snapshot(output):
    """Format/count observation only; same-UID bytes are not authenticated."""
    if type(output) is not bytes or not 0 < len(output) <= 65536:
        raise Rejected('unavailable/beyond-bound publication output snapshot')
    pattern = (rb'^test result: ok\. ([0-9]+) passed; ([0-9]+) failed; '
               rb'([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out; '
               rb'finished in [0-9.]+s\r?$')
    lines = re.findall(rb'^test result:.*$', output, re.MULTILINE)
    matches = re.findall(pattern, output, re.MULTILINE)
    if len(lines) != 1 or len(matches) != 1 or matches[0] != (b'1', b'0', b'0', b'0', b'0'):
        raise Rejected('unexpected publication inventory/result counts')
    return dict(passed=1, failed=0, ignored=0, measured=0, filtered_out=0)


def publication_counts_from_held_fd(fd):
    """No path/filename accepted; never open child-controlled output names.

    Unwired source-only observation helper. It cannot authorize or complete a
    run. A size bound limits one read, not output growth or byte authenticity.
    """
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= 65536:
        raise Rejected('invalid held publication output descriptor/snapshot size')
    output = os.pread(fd, info.st_size, 0)
    if len(output) != info.st_size:
        raise Rejected('publication output shortened during snapshot read')
    return _check_publication_count_snapshot(output)


def admit_publication(entry_id, approval_reference):
    """Non-operational public gate: fixed ID + opaque trusted-parent reference.

    No receipt/digest/allowed dictionary parameter and no same-UID file fetch.
    Future trusted-parent channel + replay/expiry/build/startup gates need their
    own implementation/review grants. There is no enabled branch in this code.
    """
    publication_policy(entry_id)
    if type(approval_reference) is not str or not re.fullmatch('approval:[0-9a-f]{32}', approval_reference):
        raise Rejected('missing opaque trusted-parent approval reference')
    raise Rejected('real publication admission disabled; trusted-parent and operational attestations unavailable')


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
