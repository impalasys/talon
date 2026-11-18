#!/usr/bin/env python3
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
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
        for relative in ['BUILD.bazel', 'crates/storage/postgres/BUILD.bazel', 'new/BUILD']:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        with patch.object(ide, 'run', return_value='BUILD.bazel\0crates/storage/postgres/BUILD.bazel\0new/BUILD\0'):
            self.assertEqual(ide.package_scope(self.root), 'set("//:all" "//crates/storage/postgres:all" "//new:all")')

    def test_scope_skips_unstaged_deleted_build_files(self):
        (self.root / 'BUILD.bazel').touch()
        with patch.object(ide, 'run', return_value='BUILD.bazel\0deleted/BUILD.bazel\0'):
            self.assertEqual(ide.package_scope(self.root), 'set("//:all")')

    def test_environment_continuations_match_rules_rust(self):
        env = self.root / 'continued.env'
        env.write_text('MULTI=one' + '\\' + '\n\ntwo=three\n' +
                       'PAIR=trailing' + '\\\\' + '\n' +
                       'ODD=trailing' + '\\\\\\' + '\nlast\n')
        self.assertEqual(ide.parse_env(env), {'MULTI': 'one\ntwo=three',
                                            'PAIR': 'trailing\\', 'ODD': 'trailing\\\nlast'})
        env.write_text('MISSING_END=one' + '\\' + '\n')
        with self.assertRaises(ValueError):
            ide.parse_env(env)

    def test_lifecycle_lock_rejects_overlap_and_releases_on_error(self):
        with self.assertRaisesRegex(ValueError, 'probe'):
            with ide.worktree_lock(self.root):
                with self.assertRaisesRegex(RuntimeError, 'Another Rust IDE command'):
                    with ide.worktree_lock(self.root):
                        self.fail('Overlapping lifecycle commands were allowed')
                raise ValueError('probe')
        with ide.worktree_lock(self.root):
            pass

    def test_sysroot_metadata_uses_bazel_cargo_and_rejects_version_mismatch(self):
        project = {'sysroot': '/pinned/sysroot', 'sysroot_src': '/pinned/library'}
        rust_version = 'rustc 1.95.0 (hash)\nhost: aarch64-apple-darwin\n'
        with patch.object(ide, 'run', side_effect=[None, 'tools/cargo\n']), \
                patch.object(ide.subprocess, 'check_output', side_effect=[rust_version, 'cargo 1.95.0 (hash)', '{}']) as output:
            self.assertEqual(ide.load_sysroot_metadata(project, self.root, 'bazel', self.root), {})
            arguments = output.call_args_list[-1].args[0]
            self.assertEqual(arguments[:2], [str(self.root / 'tools/cargo'), 'metadata'])
            self.assertEqual(output.call_args.kwargs['env']['RUSTC'], '/pinned/sysroot/bin/rustc')
        with patch.object(ide, 'run', side_effect=[None, 'tools/cargo\n']), \
                patch.object(ide.subprocess, 'check_output', side_effect=[rust_version, 'cargo 1.94.0 (hash)']) as output:
            with self.assertRaisesRegex(RuntimeError, 'versions must match'):
                ide.load_sysroot_metadata(project, self.root, 'bazel', self.root)
            self.assertEqual(output.call_count, 2)

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

    def test_refresh_and_repeated_setup_do_not_adopt_user_edits(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        settings = self.root / '.zed/settings.json'
        settings.write_text('{"format_on_save": "on"}')
        for command in ['refresh', 'setup']:
            with self.subTest(command=command), patch.object(ide, 'refresh') as refresh:
                with self.assertRaisesRegex(RuntimeError, 'settings changed since'):
                    ide.execute(SimpleNamespace(command=command, config=None, bazel='bazel'), self.root)
                refresh.assert_not_called()
        self.assertEqual(settings.read_text(), '{"format_on_save": "on"}')
        self.assertEqual(json.loads((self.root / ide.STATE / 'installed-settings.json').read_text()), {'pilot': True})

    def test_edits_during_generation_are_not_overwritten(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        settings = self.root / '.zed/settings.json'
        tools = {'rust-analyzer.server.path': '/ra', 'rust-analyzer.procMacro.server': '/macros',
                 'rust-analyzer.rustfmt.overrideCommand': ['/rustfmt']}

        def generate(*args):
            settings.write_text('{"format_on_save": "on"}')
            return tools

        with patch.object(ide, 'refresh', side_effect=generate):
            with self.assertRaisesRegex(RuntimeError, 'changed during generation'):
                ide.execute(SimpleNamespace(command='refresh', config=None, bazel='bazel'), self.root)
        self.assertEqual(settings.read_text(), '{"format_on_save": "on"}')
        self.assertFalse((self.root / ide.STATE / 'settings-transition.json').exists())

    def test_interrupted_publication_restores_both_sides_of_transition(self):
        for failure in ['settings.json', 'installed-settings.json']:
            with self.subTest(failure=failure):
                self.prepare_restore({'existed': False, 'text': None, 'model': None})
                settings = self.root / '.zed/settings.json'
                previous = settings.read_text()
                write_json = ide.atomic_json

                def interrupt(path, value):
                    if path.name == failure:
                        raise OSError('simulated interruption')
                    write_json(path, value)

                with patch.object(ide, 'atomic_json', side_effect=interrupt):
                    with self.assertRaises(OSError):
                        ide.publish_settings(self.root, previous, {'pilot': 'new revision'})
                self.assertTrue((self.root / ide.STATE / 'settings-transition.json').exists())
                ide.restore(self.root)
                self.assertFalse(settings.exists())
                self.assertFalse((self.root / 'rust-project.json').exists())

    def test_interrupted_initial_setup_does_not_allow_foreign_edits(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        (self.root / ide.STATE / 'installed-settings.json').unlink()
        settings = self.root / '.zed/settings.json'
        settings.write_text('{"mine": true}')
        with self.assertRaises(RuntimeError):
            ide.check_settings(self.root, settings.read_text())
        with self.assertRaises(RuntimeError):
            ide.restore(self.root)
        self.assertEqual(settings.read_text(), '{"mine": true}')

    def test_restore_can_retry_after_model_cleanup_failure(self):
        for relative in ['rust-project.json', ide.STATE + '/rust-project.json']:
            with self.subTest(path=relative):
                self.prepare_restore({'existed': False, 'text': None, 'model': None})
                model = self.root / relative
                model.unlink(missing_ok=True)
                model.mkdir()
                with self.assertRaises(OSError):
                    ide.restore(self.root)
                self.assertEqual(json.loads((self.root / '.zed/settings.json').read_text()), {'pilot': True})
                model.rmdir()
                ide.restore(self.root)
                self.assertFalse((self.root / '.zed/settings.json').exists())

    def test_restore_can_retry_after_settings_restored_before_metadata_cleanup(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        installed = self.root / ide.STATE / 'installed-settings.json'
        unlink = Path.unlink

        def interrupt(path, *args, **kwargs):
            if path == installed:
                raise OSError('simulated interruption')
            return unlink(path, *args, **kwargs)

        with patch.object(Path, 'unlink', new=interrupt):
            with self.assertRaises(OSError):
                ide.restore(self.root)
        self.assertFalse((self.root / '.zed/settings.json').exists())
        ide.restore(self.root)
        self.assertFalse((self.root / ide.STATE / 'zed-settings-backup.json').exists())

    def test_interrupted_first_publication_without_installed_snapshot_can_restore(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        installed = self.root / ide.STATE / 'installed-settings.json'
        installed.unlink()
        (self.root / '.zed/settings.json').unlink()
        write_json = ide.atomic_json

        def interrupt(path, value):
            if path == installed:
                raise OSError('simulated interruption')
            write_json(path, value)

        with patch.object(ide, 'atomic_json', side_effect=interrupt):
            with self.assertRaises(OSError):
                ide.publish_settings(self.root, None, {'pilot': True})
        ide.restore(self.root)
        self.assertFalse((self.root / '.zed/settings.json').exists())

    def test_pending_transition_does_not_authorize_unknown_user_settings(self):
        self.prepare_restore({'existed': False, 'text': None, 'model': None})
        ide.atomic_json(self.root / ide.STATE / 'settings-transition.json',
                        {'before': ide.json_text({'pilot': True}), 'after': ide.json_text({'pilot': 'new'})})
        settings = self.root / '.zed/settings.json'
        settings.write_text('{"mine": true}')
        with self.assertRaises(RuntimeError):
            ide.restore(self.root)
        self.assertEqual(settings.read_text(), '{"mine": true}')


if __name__ == '__main__':
    unittest.main()
