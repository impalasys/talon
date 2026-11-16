const {test} = require('node:test');
const assert = require('node:assert/strict');
const normalize = require('../../.github/scripts/buildbuddy-report.js');
const archive = Buffer.from('UEsDBBQAAAAIANigb11lcUENSAAAAHcAAAAHAAAAcHIuanNvbqtWykhNTFGyUlBKJBIo6SgoJRUl5iVngHSlpSaWlBalggRBBsUXpRbkF2eW5BdVgmSL83NT8/NS9UsSc/LzQGoKioDCJka1AFBLAQIUAxQAAAAIANigb11lcUENSAAAAHcAAAAHAAAAAAAAAAAAAACAAQAAAABwci5qc29uUEsFBgAAAAABAAEANQAAAG0AAAAAAA==', 'base64');

function fixture(head = 'a'.repeat(40)) {
  const run = {id: 10, name: 'Talon OSS CI', event: 'pull_request_target', head_sha: 'b'.repeat(40)};
  const context = {repo: {owner: 'impalasys', repo: 'talon'}, payload: {workflow_run: run}};
  const github = {rest: {
    actions: {
      listWorkflowRunArtifacts: async () => ({data: {artifacts: [{id: 3, name: 'buildbuddy-metadata'}]}}),
      downloadArtifact: async () => ({data: archive}),
    },
    pulls: {get: async () => ({data: {
      head: {sha: head, repo: {full_name: 'someone/talon'}},
      base: {repo: {full_name: 'impalasys/talon'}},
    }})},
  }};
  return {github, context};
}

test('relay metadata identifies the fork PR rather than the workflow base commit', async () => {
  const input = fixture();
  assert.equal(await normalize(input), true);
  assert.equal(input.context.payload.workflow_run.head_sha, 'a'.repeat(40));
  assert.equal(input.context.payload.workflow_run.head_repository.full_name, 'someone/talon');
});

test('stale PR metadata cannot report results for a new head', async () => {
  const input = fixture('c'.repeat(40));
  assert.equal(await normalize(input), false);
  assert.equal(input.context.payload.workflow_run.head_sha, 'b'.repeat(40));
});

test('ordinary contributor workflows cannot supply privileged relay metadata', async () => {
  const input = fixture();
  input.context.payload.workflow_run.event = 'pull_request';
  assert.equal(await normalize(input), false);
});
