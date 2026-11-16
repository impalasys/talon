// Read only metadata emitted by the trusted pull_request_target artifact relay.
const {mkdtempSync, writeFileSync, rmSync} = require('node:fs');
const {tmpdir} = require('node:os');
const {join} = require('node:path');
const {execFileSync} = require('node:child_process');

module.exports = async ({github, context}) => {
  const run = context.payload.workflow_run;
  if (run.name !== 'Talon OSS CI' || run.event !== 'pull_request_target') return false;
  const {owner, repo} = context.repo;
  const {data} = await github.rest.actions.listWorkflowRunArtifacts({owner, repo, run_id: run.id, per_page: 100});
  const artifact = data.artifacts.find(a => a.name === 'buildbuddy-metadata' && !a.expired);
  if (!artifact) return false;
  const response = await github.rest.actions.downloadArtifact({owner, repo, artifact_id: artifact.id, archive_format: 'zip'});
  const bytes = Buffer.from(response.data);
  if (bytes.length > 65536) throw new Error('Oversized CI metadata');
  const directory = mkdtempSync(join(tmpdir(), 'talon-ci-meta-'));
  try {
    const zip = join(directory, 'metadata.zip');
    writeFileSync(zip, bytes);
    const metadata = JSON.parse(execFileSync('unzip', ['-p', zip, 'pr.json'], {maxBuffer: 65536, encoding: 'utf8'}));
    if (!/^[0-9a-f]{40}$/.test(metadata.head) || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(metadata.head_repository) ||
        !Number.isSafeInteger(metadata.pr) || typeof metadata.branch !== 'string') throw new Error('Invalid CI metadata');
    const {data: pr} = await github.rest.pulls.get({owner, repo, pull_number: metadata.pr});
    if (pr.head.sha !== metadata.head || pr.head.repo?.full_name !== metadata.head_repository || pr.base.repo?.full_name !== `${owner}/${repo}`) return false;
    run.head_sha = metadata.head;
    run.head_branch = metadata.branch;
    run.head_repository = {full_name: metadata.head_repository, owner: {login: metadata.head_repository.split('/')[0]}};
    return true;
  } finally {
    rmSync(directory, {recursive: true, force: true});
  }
};
