#!/usr/bin/env python3

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from check_resource_cli_support import validate  # noqa: E402


PROTO = """
message ResourceSpec {
  oneof kind {
    AgentSpec agent = 1;
    SkillSpec skill = 2;
    WorkerSpec worker = 3;
  }
}
message ResourceStatus {
  oneof kind {
    AgentStatus agent = 1;
    CommonResourceStatus skill = 2;
    WorkerStatus worker = 3;
  }
}
"""

REGISTRY = """
[[resource]]
kind = "Agent"
aliases = ["agent"]
apply_route = "legacy"
lookup_namespace = "required"
list_namespace = "default"
name_policy = "plain"
user_authorable = true
cli_lookup = true
cli_list = true

[[resource]]
kind = "Skill"
aliases = ["skill", "skills"]
apply_route = "generic"
lookup_namespace = "required"
list_namespace = "default"
name_policy = "plain"
user_authorable = true
cli_lookup = true
cli_list = true

[[resource]]
kind = "Worker"
aliases = ["worker"]
apply_route = "internal"
lookup_namespace = "system"
list_namespace = "system"
name_policy = "plain"
user_authorable = false
cli_lookup = true
cli_list = true
"""


class ResourceCliSupportCheckTests(unittest.TestCase):
    def test_valid_registry(self) -> None:
        self.assertEqual(validate(PROTO, REGISTRY), [])

    def test_missing_resource_is_reported(self) -> None:
        self.assertIn(
            "Skill: missing from proto/resources.toml",
            validate(PROTO, REGISTRY.replace('kind = "Skill"', 'kind = "Missing"', 1)),
        )

    def test_stale_resource_is_reported(self) -> None:
        stale = REGISTRY + """

[[resource]]
kind = "Missing"
aliases = ["missing"]
apply_route = "generic"
lookup_namespace = "required"
list_namespace = "default"
name_policy = "plain"
user_authorable = true
cli_lookup = true
cli_list = true
"""
        self.assertIn(
            "Missing: registry entry has no ResourceSpec/ResourceStatus arm",
            validate(PROTO, stale),
        )

    def test_internal_resource_cannot_be_user_authorable(self) -> None:
        invalid = REGISTRY.replace("user_authorable = false", "user_authorable = true", 1)
        self.assertTrue(any("Worker: user_authorable resources" in error for error in validate(PROTO, invalid)))


if __name__ == "__main__":
    unittest.main()
