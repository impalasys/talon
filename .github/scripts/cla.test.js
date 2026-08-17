"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");

const { isTrustedRepositoryContributor } = require("./cla");

function pullRequest({ association, headRepositoryId, baseRepositoryId = 42 }) {
  return {
    author_association: association,
    head: { repo: headRepositoryId == null ? null : { id: headRepositoryId } },
    base: { repo: { id: baseRepositoryId } },
  };
}

test("trusts a same-repository branch when private membership is hidden", () => {
  assert.equal(
    isTrustedRepositoryContributor(
      pullRequest({ association: "CONTRIBUTOR", headRepositoryId: 42 })
    ),
    true
  );
});

test("trusts a visible organization member contributing from a fork", () => {
  assert.equal(
    isTrustedRepositoryContributor(
      pullRequest({ association: "MEMBER", headRepositoryId: 99 })
    ),
    true
  );
});

test("requires a CLA signature from an external fork contributor", () => {
  assert.equal(
    isTrustedRepositoryContributor(
      pullRequest({ association: "CONTRIBUTOR", headRepositoryId: 99 })
    ),
    false
  );
});

test("does not trust a deleted or unavailable fork", () => {
  assert.equal(
    isTrustedRepositoryContributor(
      pullRequest({ association: "NONE", headRepositoryId: null })
    ),
    false
  );
});
