#!/usr/bin/env python3
"""Bounded maintenance tests: only synthetic ledgers beneath the chosen root."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

BASE = Path(__file__).resolve().parents[2]
SCRIPT = BASE / 'kit/scripts/ledger.py'
FIXTURE = BASE / 'gaps/runtime/G-0008.md'
spec = importlib.util.spec_from_file_location('maintenance_ledger', SCRIPT)
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='isolated-', dir=ROOT)
        self.ws = Path(self.tmp.name) / '.devteam'
        self.path = self.ws / 'gaps/runtime/G-0008.md'
        self.path.parent.mkdir(parents=True)
        self.original = FIXTURE.read_bytes()
        self.path.write_bytes(self.original)
        self.g = ledger.parse(self.original.decode())
        self.idx = {'version': 2, 'next_id': 9, 'gaps': [ledger.summary(self.g)]}
        (self.ws / 'gaps/index.json').write_text(json.dumps(self.idx))
        self.assertEqual((len(self.g['history']), len(self.g['links']), self.g['status'], self.g['rounds']),
                         (19, 12, 'closed', 3))

    def tearDown(self):
        self.tmp.cleanup()

    def args(self):
        return argparse.Namespace(ws=str(self.ws), id='G-0008', by='maintenance-test', keep=3,
                                  expected_sha256=hashlib.sha256(self.path.read_bytes()).hexdigest())

    def archive(self):
        ledger.cmd_archive_history(self.args())
        return ledger.load_gap(str(self.ws), ledger.load_index(str(self.ws)), 'G-0008')

    def test_roundtrip_show_doctor_and_idempotent(self):
        g = self.archive()
        self.assertLessEqual(self.path.stat().st_size, 5000)
        self.assertEqual(ledger.full_history(str(self.ws), g), self.g['history'])
        for key in self.g:
            if key != 'history':
                self.assertEqual(g[key], self.g[key], key)
        archive = self.ws / g['history_archive']['path']
        record = json.loads(archive.read_bytes())
        import base64
        self.assertEqual(base64.b64decode(record['source_base64']), self.original)
        before = self.path.read_bytes()
        self.archive()
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(len(list((self.ws / 'evidence/ledger-history/G-0008').glob('*.json'))), 1)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            ledger.cmd_show(argparse.Namespace(ws=str(self.ws), id='G-0008'))
        self.assertEqual(ledger.parse(out.getvalue())['history'], self.g['history'])
        ledger.cmd_doctor(argparse.Namespace(ws=str(self.ws)))

    def test_missing_and_corrupt_archive_rejected(self):
        g = self.archive()
        path = self.ws / g['history_archive']['path']
        original = path.read_bytes()
        path.chmod(0o644)
        path.write_bytes(original + b'corrupt')
        for operation in (ledger.cmd_show, ledger.cmd_doctor):
            with self.assertRaises(SystemExit):
                operation(argparse.Namespace(ws=str(self.ws), id='G-0008'))
        path.unlink()
        for operation in (ledger.cmd_show, ledger.cmd_doctor):
            with self.assertRaises(SystemExit):
                operation(argparse.Namespace(ws=str(self.ws), id='G-0008'))

    def test_crash_before_gap_commit_and_recovery(self):
        with patch.object(ledger, 'save_gap', side_effect=RuntimeError('injected pre-commit crash')):
            with self.assertRaisesRegex(RuntimeError, 'pre-commit'):
                ledger.cmd_archive_history(self.args())
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertEqual(ledger.load_index(str(self.ws)), self.idx)
        self.archive()
        self.assertEqual(len(list((self.ws / 'evidence/ledger-history/G-0008').glob('*.json'))), 1)

    def test_subsequent_events_and_compaction_preserve_entire_chain(self):
        self.archive()
        expected = list(self.g['history'])
        for n in range(55):
            with ledger.locked(str(self.ws)) as st:
                g = ledger.load_gap(str(self.ws), st['index'], 'G-0008')
                ledger.log(g, 'maintenance-test', 'comment', f'event {n}')
                expected.append(g['history'][-1])
                ledger.save_gap(str(self.ws), st, g)
        g = ledger.load_gap(str(self.ws), ledger.load_index(str(self.ws)), 'G-0008')
        self.assertEqual(ledger.full_history(str(self.ws), g), expected)
        self.assertEqual((g['links'], g['rounds'], g['status']), (self.g['links'], 3, 'closed'))
        self.assertLessEqual(self.path.stat().st_size, 5000)
        self.assertGreater(len(list((self.ws / 'evidence/ledger-history/G-0008').glob('*.json'))), 1)
        ledger.cmd_doctor(argparse.Namespace(ws=str(self.ws)))

    def test_partial_archive_rerun_refuses_and_retains_original(self):
        with patch.object(ledger, 'save_gap', side_effect=RuntimeError('pre-commit')):
            with self.assertRaises(RuntimeError):
                ledger.cmd_archive_history(self.args())
        archive = next((self.ws / 'evidence/ledger-history/G-0008').glob('*.json'))
        archive.chmod(0o644)
        archive.write_bytes(b'partial write')
        with self.assertRaises(SystemExit):
            ledger.cmd_archive_history(self.args())
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertEqual(archive.read_bytes(), b'partial write')

    def test_changed_source_between_archive_and_commit_preserved(self):
        real = ledger.archived_gap
        changed = self.original + b'\n'
        def mutate(*args, **kwargs):
            candidate = real(*args, **kwargs)
            self.path.write_bytes(changed)
            return candidate
        with patch.object(ledger, 'archived_gap', side_effect=mutate):
            with self.assertRaises(SystemExit):
                ledger.cmd_archive_history(self.args())
        self.assertEqual(self.path.read_bytes(), changed)
        self.assertEqual(ledger.load_index(str(self.ws)), self.idx)

    def test_stale_cas_and_overlarge_result_do_not_mutate(self):
        args = self.args()
        args.expected_sha256 = '0' * 64
        with self.assertRaises(SystemExit):
            ledger.cmd_archive_history(args)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.g['description'] = 'x' * 6000
        self.path.write_text(ledger.serialize(self.g))
        before = self.path.read_bytes()
        with self.assertRaises(SystemExit):
            ledger.cmd_archive_history(self.args())
        self.assertEqual(self.path.read_bytes(), before)
        self.assertFalse((self.ws / 'evidence/ledger-history').exists())

    def test_cli_reopen_preserves_chain_and_historical_rounds(self):
        self.archive()
        result = subprocess.run([os.sys.executable, '-B', str(SCRIPT), '--dir', str(self.ws),
                                 'reopen', 'G-0008', '--by', 'qa', '--note', 'synthetic reopen'],
                                capture_output=True, text=True,
                                env={'PATH': os.environ['PATH'], 'PYTHONDONTWRITEBYTECODE': '1',
                                     'HOME': str(self.ws), 'TMPDIR': self.tmp.name})
        self.assertEqual(result.returncode, 0, result.stderr)
        g = ledger.load_gap(str(self.ws), ledger.load_index(str(self.ws)), 'G-0008')
        history = ledger.full_history(str(self.ws), g)
        self.assertEqual(history[:-1], self.g['history'])
        self.assertEqual((g['rounds'], g['links']), (4, self.g['links']))
        self.assertEqual(history[-1]['action'], 'reopen')

    def test_corrupt_archive_index_refresh_refuses_without_hiding_blocker(self):
        self.archive()
        ledger.transition(argparse.Namespace(ws=str(self.ws), id='G-0008', by='qa',
                                            note='synthetic unresolved blocker'), 'reopen')
        ledger.cmd_reindex(argparse.Namespace(ws=str(self.ws)))
        before = {str(p): p.read_bytes() for p in (self.ws / 'gaps').rglob('*') if p.is_file()}
        g = ledger.load_gap(str(self.ws), ledger.load_index(str(self.ws)), 'G-0008')
        archive = self.ws / g['history_archive']['path']
        archive.chmod(0o644)
        archive.write_bytes(archive.read_bytes() + b'corrupt')
        result = subprocess.run([os.sys.executable, '-B', str(SCRIPT), '--dir', str(self.ws), 'reindex'],
                                capture_output=True, text=True,
                                env={'PATH': os.environ['PATH'], 'PYTHONDONTWRITEBYTECODE': '1',
                                     'HOME': str(self.ws), 'TMPDIR': self.tmp.name})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('history archive bytes/hash mismatch', result.stderr)
        for operation, args in (
            (ledger.cmd_render, argparse.Namespace(ws=str(self.ws))),
            (ledger.cmd_comment, argparse.Namespace(ws=str(self.ws), id='G-0008', by='qa', note='blocked')),
        ):
            with self.assertRaises(SystemExit):
                operation(args)
        self.assertEqual({str(p): p.read_bytes() for p in (self.ws / 'gaps').rglob('*') if p.is_file()}, before)
        self.assertEqual(len(ledger.load_index(str(self.ws))['gaps']), 1)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), self.assertRaises(SystemExit) as raised:
            ledger.cmd_gate(argparse.Namespace(ws=str(self.ws), phase='review', area='runtime'))
        self.assertEqual(raised.exception.code, 1)
        self.assertIn('G-0008', out.getvalue())


if __name__ == '__main__':
    ROOT = BASE / 'evidence/R011/maintenance-synthetic'
    ROOT.mkdir(exist_ok=True)
    unittest.main(verbosity=2)
