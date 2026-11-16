import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import affected_targets as ci


def rule(name, tags=(), inputs=(), outputs=()):
    return {"type": "RULE", "rule": {"name": name, "ruleClass": "go_test", "attribute": [
        {"name": "tags", "stringListValue": list(tags)},
    ], "ruleInput": list(inputs), "ruleOutput": list(outputs)}}


class SelectionTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "BUILD").touch()
        (self.root / "pkg").mkdir()
        (self.root / "pkg/BUILD.bazel").touch()
        (self.root / "pkg/auth_test.go").touch()
        self.auth = "//pkg:auth_test"
        self.other = "//other:test"
        self.records = [rule(self.auth, inputs=["//pkg:auth_test.go"]), rule(self.other), rule("//release:publish", ["manual"]),
                        {"type": "SOURCE_FILE", "sourceFile": {"name": "//pkg:auth_test.go"}}]
        self.graph = "\n".join(json.dumps(record) for record in self.records)

    def select(self, files=(), targets=(), full=False, platform=None):
        return ci.select_targets(self.root, "bazel", list(files), list(targets), platform, full)

    def test_new_untagged_test_is_discovered_without_registration(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(full=True), [self.other, self.auth])

    def test_manual_target_remains_available_to_explicit_caller(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(full=True, targets=["//release:publish"]), ["//release:publish"])

    def test_changed_test_source_selects_its_test(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(["pkg/auth_test.go"]), [self.auth])

    def test_reporting_follows_generated_files_and_aliases_without_external_loading(self):
        mac = "//desktop:mac_dmg"
        self.records.extend([
            rule("//pkg:generate", inputs=["//pkg:auth_test.go"], outputs=["//pkg:generated.swift"]),
            rule("//pkg:alias", inputs=["//pkg:generated.swift"]),
            rule(mac, ["manual"], inputs=["//pkg:alias", "@swiftpkg//:metadata"]),
        ])
        graph = "\n".join(json.dumps(record) for record in self.records)
        # A dependency query would need host-specific external metadata. Only
        # the workspace inventory is available to a cross-host reporting job.
        with patch.object(ci, "bazel_output", side_effect=[graph]):
            self.assertEqual(self.select(["pkg/auth_test.go"], targets=[mac, self.other]), [mac])

    def test_reporting_excludes_unrelated_requested_roots(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(["pkg/auth_test.go"], targets=[self.other]), [])

    def test_deleted_source_uses_complete_universe(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(["pkg/deleted_test.go"]), [self.other, self.auth])

    def test_linux_changed_source_selects_configured_dependents(self):
        configured = json.dumps({"results": [{"target": record} for record in self.records[:2]]})
        affected = json.dumps({"results": [{"target": rule(self.auth)}]})
        with patch.object(ci, "bazel_output", side_effect=[self.graph, configured, "", affected]):
            self.assertEqual(self.select(["pkg/auth_test.go"], platform="//:linux_amd64"), [self.auth])

    def test_linux_changed_source_with_no_configured_dependents_selects_nothing(self):
        configured = json.dumps({"results": [{"target": record} for record in self.records[:2]]})
        for empty_graph in ["{}", '{"results":[]}']:
            with self.subTest(graph=empty_graph), patch.object(
                ci, "bazel_output", side_effect=[self.graph, configured, "", empty_graph],
            ):
                self.assertEqual(self.select(["pkg/auth_test.go"], platform="//:linux_amd64"), [])

    def test_build_bazel_change_uses_complete_universe(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(["pkg/BUILD.bazel"]), [self.other, self.auth])

    def test_configuration_changes_include_android_and_new_targets(self):
        for path in ["MODULE.bazel", "MODULE.bazel.lock", ".bazelversion", ".bazelrc.local", "rules.bzl", "scripts/ci/runner.sh",
                     "Cargo.lock", "Cargo.toml", "tools/bazel/requirements.lock", "tools/bazel/maven_install.json",
                     "go.work", "go.work.sum", "sdk/go/talon-client/go.mod", "sdk/go/talon-client/go.sum", ".bazelignore"]:
            file = self.root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.touch()
            with self.subTest(path=path), patch.object(ci, "bazel_output", return_value=self.graph):
                self.assertEqual(self.select([path]), [self.other, self.auth])

    def test_unowned_document_does_not_run_unrelated_checks(self):
        (self.root / "README.md").touch()
        with patch.object(ci, "bazel_output", return_value=self.graph):
            self.assertEqual(self.select(["README.md"]), [])

    def test_no_changes_selects_nothing(self):
        with patch.object(ci, "bazel_output", side_effect=AssertionError("unexpected Bazel call")):
            self.assertEqual(self.select(), [])

    def test_linux_analysis_filters_incompatible_targets(self):
        configured = json.dumps({"results": [{"target": record} for record in self.records[:2]]})
        with patch.object(ci, "bazel_output", side_effect=[self.graph, configured, self.other + "\n"]):
            self.assertEqual(self.select(full=True, platform="//:linux_amd64"), [self.auth])

    def test_linux_analysis_with_no_compatible_targets_selects_nothing(self):
        for empty_graph in ["{}", '{"results":[]}']:
            with self.subTest(graph=empty_graph), patch.object(
                ci, "bazel_output", side_effect=[self.graph, empty_graph, ""],
            ):
                self.assertEqual(self.select(full=True, platform="//:linux_amd64"), [])

    def test_compatible_alias_keeps_its_workspace_name(self):
        name = "//sdk:python"
        configured = json.dumps({"results": [{"target": rule(name)}]})
        with patch.object(ci, "bazel_output", side_effect=[configured, ""]):
            self.assertEqual(ci.compatible_targets("bazel", [name], "//:linux_amd64"), [name])

    def test_unconfigured_test_suite_is_replaced_by_its_configured_members(self):
        configured = json.dumps({"results": [{"target": rule(self.auth)}]})
        with patch.object(ci, "bazel_output", side_effect=[configured, ""]):
            self.assertEqual(ci.compatible_targets("bazel", ["//pkg:suite"], "//:linux_amd64"), [self.auth])

    def test_invalid_configured_graph_does_not_skip_ci(self):
        for graph in [
            "", "not json", "[]", "null", '{"unexpected":[]}',
            '{"results":null}', '{"results":{}}', '{"results":""}',
            '{"results":[{"target":{"type":"UNKNOWN"}}]}',
        ]:
            with self.subTest(graph=graph), patch.object(ci, "bazel_output", return_value=graph):
                with self.assertRaises(ci.SelectionError):
                    ci.compatible_targets("bazel", [self.auth], "//:linux_amd64")

    def test_query_failure_is_not_an_empty_selection(self):
        result = subprocess.CompletedProcess(["bazel"], 7, "", "dependency load failed\n")
        with patch.object(subprocess, "run", return_value=result):
            with self.assertRaises(ci.SelectionError):
                self.select(full=True)

    def test_configured_query_failure_is_not_an_empty_selection(self):
        configured = json.dumps({"results": [{"target": record} for record in self.records[:2]]})
        responses = [
            subprocess.CompletedProcess(["bazel"], 0, self.graph),
            subprocess.CompletedProcess(["bazel"], 0, configured),
            subprocess.CompletedProcess(["bazel"], 0, ""),
            subprocess.CompletedProcess(["bazel"], 7, "{}"),
        ]
        with patch.object(subprocess, "run", side_effect=responses):
            with self.assertRaises(ci.SelectionError):
                self.select(["pkg/auth_test.go"], platform="//:linux_amd64")

    def test_invalid_graph_is_not_an_empty_selection(self):
        for graph in ["", "not json", '{"type":"RULE"}']:
            with self.subTest(graph=graph), patch.object(ci, "bazel_output", return_value=graph):
                with self.assertRaises(ci.SelectionError):
                    self.select(full=True)

    def test_missing_explicit_target_fails(self):
        with patch.object(ci, "bazel_output", return_value=self.graph):
            with self.assertRaises(ci.SelectionError):
                self.select(full=True, targets=["//missing:test"])

    def test_sources_resolve_against_nearest_package(self):
        self.assertEqual(ci.source_label(self.root, "pkg/auth_test.go"), "//pkg:auth_test.go")
        self.assertEqual(ci.source_label(self.root, "README.md"), "//:README.md")

    def test_labels_with_spaces_are_kept_intact(self):
        label = "//some package:unit test"
        self.assertEqual(ci.output_labels(label + "\n"), [label])
        self.assertEqual(json.loads(ci.label_set([label])[4:-1]), label)



if __name__ == "__main__":
    unittest.main()
