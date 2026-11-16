# Linux CI operations

Linux validation runs in one **Talon CI** BuildBuddy Workflow, configured in
[`buildbuddy.yaml`](../../buildbuddy.yaml). It requests 16 CPUs, 32 GiB RAM and
80 GiB disk on Ubuntu 24.04. The JVM has a 4-GiB heap; Bazel schedules at most
16 jobs with a 20-GiB action-memory budget. Remote execution is disabled.

## PR and main behavior

- PR events merge the PR with its current main base before validation.
- PR selection follows configured Bazel reverse dependencies of changed source
  files, including generated-file consumers and tests. A shared Rust-library
  edit can legitimately affect most runtime targets.
- BUILD files, `.bzl`, module/lock/toolchain files, CI configuration and deleted
  files select the full compatible universe. Failed queries fail CI.
- Main revisions complete independently so an SDK publication can wait for its
  exact commit even if another main push arrives. Superseded PR runs cancel.
- Main pushes build every ordinary Linux-compatible target and run every
  ordinary compatible test. Test results are forced uncached in both modes.
- Docker sandbox tests, ACP/runtime image smoke tests, Python E2E, the complete
  Sightline browser suite, chat screenshots, documentation checks, release
  archive checks and the 1-vCPU benchmark are ordinary targets. New tests are
  discovered from Bazel without maintaining a workflow target list.
- Process-level durability/restart stress tests are included on Linux too.
  Only the native npm archive aliases remain manual packaging helpers for the
  staging steps in GitHub publishing.

Bazel's `exclusive` tags serialize the integration and benchmark suites that
share Docker resources/ports. The benchmark keeps its existing 1-vCPU container
limits; throughput thresholds remain advisory, but missing results fail CI.
Moving its host means historical throughput comparisons need a new baseline.

## Artifacts and GitHub responsibilities

The runner writes `ci-manifest.json`, profiles, test logs/XML, and `handoff.tar`
to `BUILDBUDDY_ARTIFACTS_DIRECTORY`. The manifest identifies the PR head, tested
base, built tree, selected targets/tests and checksums of transferred files.
Only outputs selected in that run are exported; old snapshot screenshots are
not reused as results of an unrelated PR.

GitHub's **Talon OSS CI** is now an artifact relay and publisher. Its
`pull_request_target` job checks out only the trusted base commit and never
executes PR code. It uses the existing BuildBuddy credential to read artifacts,
validates the status publisher/context and current PR revision, compares the
built tree to GitHub's merge tree, and verifies CAS/member hashes. Links,
traversal paths, missing files and stale results fail the handoff. The GitHub
token in that job is read-only. No credential is created or sent to PR builds.

GitHub retains:

- macOS native release builds and verification;
- main-only Linux release packaging, which preserves the existing desktop OAuth
  compile-time configuration without transferring those values to BuildBuddy;
- publishing the exact tested OCI image bytes, Cloudflare exports and Storybook;
- SDK package publishing (including native macOS SDK binaries);
- CLA checks and PR screenshot/benchmark reports.

Linux SDK release assets come from the verified BuildBuddy handoff. Package
publishers wait for successful full CI of the exact main commit. Screenshot and
benchmark artifact names remain compatible with PR reports; trusted relay
metadata identifies the original PR even for fork builds.

There is no GitHub Linux validation fallback or provider-selection variable.
BuildBuddy reports the `Talon CI` status directly. The old pilot and CPU-sweep
actions are no longer automatically configured. Historical measurements remain
in `bench/linux-ci-*-2026-11-15.json` and the assessment.

## Diagnosis and rollback

Open the `Talon CI` status on the failing commit. Inspect `ci-manifest.json` for
selection and per-phase results, then the corresponding test log/profile.
A query/analysis failure must be fixed or rerun; do not convert it into an empty
selection. An outdated artifact relay should be rerun for the current PR/base.

For local selection diagnosis on Linux:

```sh
python3 scripts/ci/affected_targets.py --full --platform=//:linux_ci
```

For a full runner qualification in the BuildBuddy Linux environment:

```sh
python3 scripts/ci/linux_build.py --full --output /tmp/talon-ci
```

Rollback is a reviewed revert of the consolidation commit, restoring the
previous workflows together with their artifact producers. Do not leave a
publisher consuming unverified or mutable latest artifacts during a rollback.

<!-- Temporary documentation-only edit for affected-target CI verification. -->
