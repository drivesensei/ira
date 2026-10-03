import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
import isolated_test_binary as launcher


class Safety(unittest.TestCase):
    def setUp(self):
        # Every test root is retained. /tmp is canonicalized once by caller.
        self.parent = Path(tempfile.mkdtemp(prefix='t049-', dir='/tmp')).resolve()
        self.parent.chmod(0o700)
        print('retained fixture: ' + str(self.parent), flush=True)
        self.script = self.parent / 'probe.c'
        self.script.write_text('#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n#include <unistd.h>\n#include <sys/wait.h>\nint main(int n,char**v){if(n>1 && !strcmp(v[1],"detach")){if(fork()==0){setsid();close(1);close(2);usleep(200000);}return 0;}if(n>1 && !strcmp(v[1],"timeout")){usleep(200000);return 0;}if(n>1 && !strcmp(v[1],"exit7"))return 7; puts(getenv("HOME"));puts(getenv("TMPDIR"));puts(getenv("IRA_THUMBNAIL_CACHE_DIR"));puts(getenv("PWD"));fflush(stdout);if(fork()==0){puts(getenv("HOME"));return 0;}wait(NULL);return 0;}\n')
        self.binary = self.parent / 'probe'
        self.compiler = Path('/usr/bin/clang').resolve()
        import subprocess
        build = [str(self.compiler), str(self.script), '-o', str(self.binary)]
        subprocess.run(build, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.receipt = dict(target='synthetic-probe', kind='synthetic', source_revision='fixture-v1',
                            build_argv=build, argv=[str(self.binary)],
                            artifacts={'binary': self.artifact(self.binary), 'compiler': self.artifact(self.compiler),
                                       'sources': [self.artifact(self.script)]})
        self.pin()

    def artifact(self, path):
        return {'path': str(path), 'sha256': launcher.digest(path)}

    def pin(self):
        self.sha = hashlib.sha256(json.dumps(self.receipt, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        self.allowed = {'synthetic-probe': self.sha}

    def launch(self, **kwargs):
        return launcher.run(self.receipt, self.sha, self.allowed, self.parent, 'run', **kwargs)

    def test_child_and_nested_environment_parent_unchanged(self):
        before = dict(os.environ)
        result = self.launch()
        self.assertEqual(before, dict(os.environ))
        self.assertEqual(result['exit_code'], 0)
        lines = (self.parent / 'run/stdout').read_text().splitlines()
        self.assertEqual(lines, [str(self.parent / 'run' / n) for n in ('home', 'tmp', 'cache', 'cwd', 'home')])
        self.assertEqual(result['status'], 'INCOMPLETE')
        self.assertEqual(set(result['environment']), {'LANG', 'LC_ALL', 'TZ', 'HOME', 'TMPDIR', 'IRA_THUMBNAIL_CACHE_DIR', 'PWD'})
        for directory in result['directories'].values():
            self.assertEqual(directory['mode'], '0o700')

    def test_preexisting_namespace_rejected(self):
        (self.parent / 'run').mkdir()
        with self.assertRaises(launcher.Rejected): self.launch()

    def test_symlink_namespace_rejected(self):
        (self.parent / 'run').symlink_to(self.parent, target_is_directory=True)
        with self.assertRaises(launcher.Rejected): self.launch()

    def test_relative_parent_rejected(self):
        with self.assertRaises(launcher.Rejected):
            launcher.run(self.receipt, self.sha, self.allowed, '.', 'run')

    def test_unsafe_parent_permissions_rejected(self):
        self.parent.chmod(0o755)
        with self.assertRaises(launcher.Rejected): self.launch()
        self.assertFalse((self.parent / 'run').exists())

    def test_symlink_parent_rejected(self):
        alias = self.parent / 'alias'
        alias.symlink_to(self.parent, target_is_directory=True)
        with self.assertRaises(launcher.Rejected):
            launcher.run(self.receipt, self.sha, self.allowed, alias, 'run')

    def test_unsafe_environment_rejected_before_namespace(self):
        for overrides in ({'PATH': '/bin'}, {'HOME': '/tmp'}, {'IRA_WT_SETTINGS': '/tmp/x'}, {'LANG': 'secret'}):
            with self.assertRaises(launcher.Rejected): self.launch(overrides=overrides)
        self.assertFalse((self.parent / 'run').exists())

    def test_source_tamper_rejected(self):
        self.script.write_text('exit 0\n')
        with self.assertRaises(launcher.Rejected): self.launch()
        self.assertFalse((self.parent / 'run').exists())

    def test_binary_tamper_rejected(self):
        binary = self.parent / 'sh'
        binary.write_bytes(self.binary.read_bytes())
        self.receipt['argv'][0] = str(binary)
        self.receipt['artifacts']['binary'] = self.artifact(binary)
        self.pin()
        binary.write_bytes(b'tamper')
        with self.assertRaises(launcher.Rejected): self.launch()

    def test_unexpected_target_source_and_toolchain_receipts(self):
        for key in ('source_revision', 'target'):
            original = self.receipt[key]
            self.receipt[key] = 'unexpected'
            with self.assertRaises(launcher.Rejected): self.launch()
            self.receipt[key] = original
        self.receipt['artifacts']['compiler']['sha256'] = '0' * 64
        self.pin()
        with self.assertRaises(launcher.Rejected): self.launch()

    def test_real_target_disabled_even_with_pin(self):
        self.receipt.update(target='operations_test', kind='original')
        self.pin()
        self.allowed = {'operations_test': self.sha}
        with self.assertRaises(launcher.Rejected): self.launch()

    def test_timeout_retains_no_false_completion(self):
        self.receipt['argv'].append('timeout')
        self.pin()
        result = self.launch(timeout=0.01)
        self.assertIsNone(result['exit_code'])
        self.assertEqual(result['status'], 'INCOMPLETE')
        self.assertTrue((self.parent / 'run/receipt.json').exists())
        self.assertEqual(result['descendants'], 'UNVERIFIED')

    def test_detached_descendant_never_claims_completion(self):
        self.receipt['argv'].append('detach')
        self.pin()
        result = self.launch()
        self.assertEqual(result['exit_code'], 0)
        self.assertEqual(result['status'], 'INCOMPLETE')
        self.assertEqual(result['descendants'], 'UNVERIFIED')
        self.assertTrue((self.parent / 'run/home').exists())

    def attack(self, operation):
        import subprocess
        attack_source = self.parent / 'attack.c'
        attack_binary = self.parent / 'attack'
        attack_source.write_text(r'''#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
int main(int n, char **v) {
    if(n != 3) return 11;
    if(!strcmp(v[1], "root")) {
        if(rename("../../run", "../../run.retained")) return 12;
        if(symlink(v[2], "../../run")) return 13;
    } else if(!strcmp(v[1], "directory")) {
        if(rename("../home", "../home.retained")) return 14;
        if(symlink(v[2], "../home")) return 15;
    } else if(!strcmp(v[1], "regular")) {
        if(rename("../receipt.json", "../receipt.retained.json")) return 16;
        FILE *f = fopen("../receipt.json", "wx");
        if(!f) return 17;
        fputs("child replacement\n", f); fclose(f);
    } else {
        char original[64], retained[64];
        snprintf(original, sizeof(original), "../%s", v[1]);
        snprintf(retained, sizeof(retained), "../%s.retained", v[1]);
        if(rename(original, retained)) return 18;
        if(symlink(v[2], original)) return 19;
    }
    puts("child original output"); fflush(stdout);
    return 0;
}
''')
        build = [str(self.compiler), str(attack_source), '-o', str(attack_binary)]
        subprocess.run(build, check=True, capture_output=True)
        sentinel = self.parent / 'sentinel'
        sentinel.write_bytes(b'owned outside-run sentinel\n')
        if operation in ('root', 'directory'):
            sentinel_dir = self.parent / 'sentinel-directory'
            sentinel_dir.mkdir(mode=0o700)
            # Victim receipt/output names are all owned benign sentinel fixtures.
            for name in ('receipt.json', 'stdout', 'stderr'):
                (sentinel_dir / name).write_bytes(b'owned directory sentinel\n')
            argument = sentinel_dir
            files = list(sentinel_dir.iterdir())
        else:
            argument, files = sentinel, [sentinel]
        before = {str(f): launcher.digest(f) for f in files}
        self.receipt.update(build_argv=build, argv=[str(attack_binary), operation, str(argument)])
        self.receipt['artifacts']['binary'] = self.artifact(attack_binary)
        self.receipt['artifacts']['sources'] = [self.artifact(attack_source)]
        self.pin()
        error = None
        try:
            self.launch()
        except launcher.Rejected as exc:
            error = exc
        after = {str(f): launcher.digest(f) for f in files}
        # Check sentinel safety before rejection so RED identifies real mutation.
        self.assertEqual(before, after, 'launcher touched outside-run sentinel')
        self.assertIsNotNone(error, 'launcher failed to reject child replacement')
        retained_root = self.parent / ('run.retained' if operation == 'root' else 'run')
        retained_receipt = retained_root / ('receipt.retained.json' if operation == 'regular' else
                                           'receipt.json.retained' if operation == 'receipt.json' else 'receipt.json')
        state = json.loads(retained_receipt.read_text())
        self.assertEqual(state['status'], 'INCOMPLETE')
        self.assertEqual(state['namespace_integrity'], 'COMPROMISED')
        self.assertTrue(state['authority_errors'])
        # Child original stdout is hashed by retained FD, never the sentinel path.
        output = retained_root / ('stdout.retained' if operation == 'stdout' else 'stdout')
        self.assertEqual(state['stdout_sha256'], launcher.digest(output))
        stderr = retained_root / ('stderr.retained' if operation == 'stderr' else 'stderr')
        self.assertEqual(state['stderr_sha256'], launcher.digest(stderr))
        if operation in ('stdout', 'stderr'):
            self.assertNotEqual(state[operation + '_sha256'], launcher.digest(sentinel))
        self.assertEqual(state['exit_code'], 0)

    def test_receipt_symlink_cannot_redirect_parent_write(self):
        self.attack('receipt.json')

    def test_receipt_regular_replacement_detected(self):
        self.attack('regular')

    def test_stdout_symlink_cannot_redirect_parent_hash(self):
        self.attack('stdout')

    def test_stderr_symlink_cannot_redirect_parent_hash(self):
        self.attack('stderr')

    def test_owned_directory_replacement_detected(self):
        self.attack('directory')

    def test_run_root_replacement_cannot_redirect_parent_write(self):
        self.attack('root')

    def test_nonzero_exit_honest(self):
        self.receipt['argv'].append('exit7')
        self.pin()
        self.assertEqual(self.launch()['exit_code'], 7)


if __name__ == '__main__':
    unittest.main(verbosity=2)
