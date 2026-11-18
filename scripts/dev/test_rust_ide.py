#!/usr/bin/env python3
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import rust_ide as ide


class RustIdeTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_settings_disable_automatic_builds_and_preserve_preferences(self):
        old = {'format_on_save': 'off', 'lsp': {'other': {'enabled': True}, 'rust-analyzer': {
            'binary': {'env': {'RA_LOG': 'info'}},
            'initialization_options': {'cargo': {'features': ['aws'], 'buildScripts': {'overrideCommand': ['old']}},
                                       'workspace': {'symbol': {'search': {'kind': 'all_symbols'}}, 'discoverConfig': {'command': ['old']}},
                                       'diagnostics': {'disabled': ['inactive-code']}}}}}
        before = json.loads(json.dumps(old))
        tools = {'rust-analyzer.server.path': '/bazel/ra', 'rust-analyzer.procMacro.server': '/bazel/macros',
                 'rust-analyzer.rustfmt.overrideCommand': ['/bazel/rustfmt']}
        result = ide.zed_settings(old, self.root, tools)
        init = result['lsp']['rust-analyzer']['initialization_options']
        self.assertEqual(old, before)
        self.assertEqual(result['lsp']['rust-analyzer']['binary']['env'], {'RA_LOG': 'info'})
        self.assertEqual(init['linkedProjects'], [str(self.root / ide.STATE / 'rust-project.json')])
        self.assertFalse(init['checkOnSave'])
        self.assertFalse(init['cargo']['autoreload'])
        self.assertFalse(init['cargo']['buildScripts']['enable'])
        self.assertFalse(init['cargo']['buildScripts']['rebuildOnSave'])
        self.assertEqual(init['cargo']['features'], ['aws'])
        self.assertIsNone(init['workspace']['discoverConfig'])
        self.assertIn('symbol', init['workspace'])
        self.assertTrue(init['procMacro']['enable'])
        self.assertEqual(result['lsp']['other'], old['lsp']['other'])
        self.assertEqual(result['format_on_save'], 'off')

    def test_model_uses_bazel_env_file_without_changing_other_crates(self):
        execution = self.root / 'exec'
        execution.mkdir()
        (execution / 'version.env').write_text('CARGO_PKG_VERSION=0.1.0\nVALUE=one=two\n')
        project = {'crates': [{'root_module': str(self.root / 'src/lib.rs'), 'env': {'CARGO_PKG_VERSION': '0.0.0', 'OUT_DIR': '/generated'}},
                              {'root_module': '/other/lib.rs', 'env': {'CARGO_PKG_VERSION': '0.0.0'}}],
                   'runnables': [{'kind': 'Flycheck'}, {'kind': 'testOne'}, {'kind': 'run'}]}
        result = ide.finish_model(project, [{'root': 'src/lib.rs', 'source': True, 'env_files': ['version.env']}], self.root, execution)
        self.assertEqual(result['crates'][0]['env'], {'CARGO_PKG_VERSION': '0.1.0', 'VALUE': 'one=two', 'OUT_DIR': '/generated'})
        self.assertEqual(result['crates'][1]['env']['CARGO_PKG_VERSION'], '0.0.0')
        self.assertEqual([r['kind'] for r in result['runnables']], ['testOne', 'run'])

    def test_missing_or_malformed_env_files_fail(self):
        with self.assertRaises(FileNotFoundError):
            ide.parse_env(self.root / 'missing')
        bad = self.root / 'bad'
        bad.write_text('not an assignment\n')
        with self.assertRaises(ValueError):
            ide.parse_env(bad)

    def test_scope_excludes_ignored_worktrees_and_includes_new_packages(self):
        with patch.object(ide, 'run', return_value='BUILD.bazel\0crates/storage/postgres/BUILD.bazel\0new/BUILD\0'):
            self.assertEqual(ide.package_scope(self.root), 'set("//:all" "//crates/storage/postgres:all" "//new:all")')

    def test_sysroot_contains_source_dependencies_without_cargo_discovery(self):
        packages, nodes = [], []
        for name in ['core', 'alloc', 'std', 'test', 'proc_macro']:
            packages.append({'id': name, 'edition': '2024', 'version': '0.0.0', 'manifest_path': f'/sysroot/{name}/Cargo.toml',
                             'targets': [{'name': name, 'kind': ['lib'], 'src_path': f'/sysroot/{name}/src/lib.rs'}]})
            deps = [] if name == 'core' else [{'pkg': 'core', 'name': 'core', 'dep_kinds': [{'kind': None}]}]
            nodes.append({'id': name, 'features': [], 'deps': deps})
        result = ide.sysroot_model({'packages': packages, 'resolve': {'nodes': nodes}})
        self.assertEqual(len(result['crates']), 5)
        self.assertEqual(result['crates'][2]['deps'], [{'crate': 0, 'name': 'core'}])
        self.assertTrue(all(not c['is_workspace_member'] for c in result['crates']))
        self.assertNotIn('sysroot_src', result)
        with self.assertRaises(RuntimeError):
            ide.sysroot_model({'packages': [], 'resolve': {'nodes': []}})

    def prepare_restore(self, original):
        ide.atomic_json(self.root / ide.STATE / 'zed-settings-backup.json', original)
        ide.atomic_json(self.root / '.zed/settings.json', {'pilot': True})
        ide.atomic_json(self.root / ide.STATE / 'installed-settings.json', {'pilot': True})
        (self.root / 'rust-project.json').write_text('generated')

    def test_restore_removes_generated_model_so_cargo_discovery_can_resume(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        ide.restore(self.root)
        self.assertFalse((self.root / '.zed/settings.json').exists())
        self.assertFalse((self.root / 'rust-project.json').exists())

    def test_restore_preserves_original_settings_and_model(self):
        self.prepare_restore({'existed': True, 'text': '{"original": true}\n', 'model': '{"old": "model"}'})
        ide.restore(self.root)
        self.assertEqual((self.root / '.zed/settings.json').read_text(), '{"original": true}\n')
        self.assertEqual((self.root / 'rust-project.json').read_text(), '{"old": "model"}')

    def test_restore_refuses_to_overwrite_subsequent_user_edits(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        (self.root / '.zed/settings.json').write_text('{"mine": true}')
        with self.assertRaises(RuntimeError):
            ide.restore(self.root)
        self.assertEqual((self.root / '.zed/settings.json').read_text(), '{"mine": true}')


if __name__ == '__main__':
    unittest.main()
