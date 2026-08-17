"use strict";

const ORGANIZATION_ASSOCIATIONS = new Set(["MEMBER", "OWNER"]);

function isTrustedRepositoryContributor(pullRequest) {
  const headRepositoryId = pullRequest.head?.repo?.id;
  const baseRepositoryId = pullRequest.base?.repo?.id;
  const usesTrustedRepositoryBranch =
    headRepositoryId != null && headRepositoryId === baseRepositoryId;

  return (
    usesTrustedRepositoryBranch ||
    ORGANIZATION_ASSOCIATIONS.has(pullRequest.author_association)
  );
}

module.exports = { isTrustedRepositoryContributor };
