// Contributor Licence Agreement check for pull requests (TD-148).
//
// Run by .github/workflows/cla.yml through actions/github-script, always from the default
// branch: the pull request's own code is never checked out or run. Every author of the pull
// request's commits must have accepted CLA.md by posting ACCEPTANCE as a comment; acceptances are
// stored in the private repository named by SIGNATURES, written with the CLA_SIGNATURES_TOKEN
// secret. The check sets the commit status STATUS_CONTEXT on the pull request's head and keeps one
// comment, marked MARKER, that says who still has to accept. A merged pull request is locked, so
// that the comments recording acceptances stay as they were.
//
// Tests: node --test .github/scripts/cla.test.js
'use strict';

const SIGNATURES = {
  owner: 'dazeroit',
  repo: 'karasu-slicer-cla',
  path: 'signatures/cla-v1.json',
  branch: 'main',
};
const CLA_VERSION = 1;
const ACCEPTANCE =
  'I have read the karasu-slicer Contributor Licence Agreement (CLA.md, version 1) and I agree ' +
  'to its terms for all my contributions to karasu-slicer.';
const RECHECK = 'recheck';
const MARKER = '<!-- karasu-cla -->';
const STATUS_CONTEXT = 'CLA';
// The Owner's own account needs no acceptance; bots cannot accept.
const ALLOWLIST = ['skullab'];

/** The text with its spaces made single and its ends trimmed. */
function normalize(text) {
  return String(text || '').trim().replace(/\s+/g, ' ');
}

/** Whether a login needs no acceptance. */
function exempt(login) {
  return ALLOWLIST.includes(login) || login.endsWith('[bot]');
}

/** The pull request the event is about. */
async function pullRequest(github, context) {
  if (context.payload.pull_request) {
    return context.payload.pull_request;
  }
  const { data } = await github.rest.pulls.get({
    ...context.repo,
    pull_number: context.payload.issue.number,
  });
  return data;
}

/**
 * The authors of the pull request's commits, by GitHub user id, with the name and e-mail address
 * of their first commit, and the commits whose author is not linked to a GitHub account.
 */
async function authors(github, context, number) {
  const commits = await github.paginate(github.rest.pulls.listCommits, {
    ...context.repo,
    pull_number: number,
    per_page: 100,
  });
  const people = new Map();
  const unlinked = [];
  for (const commit of commits) {
    const user = commit.author;
    if (!user || !user.login) {
      unlinked.push(commit.sha.slice(0, 7));
      continue;
    }
    if (!people.has(user.id)) {
      people.set(user.id, {
        login: user.login,
        id: user.id,
        name: commit.commit.author.name,
        email: commit.commit.author.email,
      });
    }
  }
  return { people, unlinked };
}

/** The stored acceptances and the file's blob identity (null when there is no file yet). */
async function readStore(store) {
  try {
    const { data } = await store.rest.repos.getContent({
      owner: SIGNATURES.owner,
      repo: SIGNATURES.repo,
      path: SIGNATURES.path,
      ref: SIGNATURES.branch,
    });
    const content = JSON.parse(Buffer.from(data.content, 'base64').toString('utf8'));
    return { content, sha: data.sha };
  } catch (error) {
    if (error.status === 404) {
      return {
        content: {
          agreement: 'karasu-slicer Contributor Licence Agreement',
          version: CLA_VERSION,
          signatures: [],
        },
        sha: null,
      };
    }
    throw error;
  }
}

/** Store one acceptance, retrying when another run wrote the file meanwhile. */
async function addSignature(store, entry) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const { content, sha } = await readStore(store);
    if (content.signatures.some((s) => s.id === entry.id)) {
      return false;
    }
    content.signatures.push(entry);
    try {
      await store.rest.repos.createOrUpdateFileContents({
        owner: SIGNATURES.owner,
        repo: SIGNATURES.repo,
        path: SIGNATURES.path,
        branch: SIGNATURES.branch,
        message: `Record ${entry.login}'s acceptance of CLA version ${CLA_VERSION}`,
        content: Buffer.from(JSON.stringify(content, null, 2) + '\n').toString('base64'),
        ...(sha ? { sha } : {}),
      });
      return true;
    } catch (error) {
      if (error.status !== 409 && error.status !== 422) {
        throw error;
      }
    }
  }
  throw new Error('the acceptances file kept changing; post "recheck" to try again');
}

/** The comment that says who still has to accept, or that everyone has. */
function message(link, pending, unlinked) {
  if (pending.length === 0 && unlinked.length === 0) {
    return `${MARKER}\nEvery author of this pull request has accepted the [Contributor Licence ` +
      `Agreement](${link}). Thank you.`;
  }
  const lines = [
    MARKER,
    `Thank you for your contribution. Before it can be merged, every author of its commits must ` +
      `accept the [karasu-slicer Contributor Licence Agreement](${link}).`,
  ];
  if (pending.length > 0) {
    lines.push(
      '',
      'To accept it, post this comment in this pull request, exactly:',
      '',
      `> ${ACCEPTANCE}`,
      '',
      `Still to accept: ${pending.map((p) => `@${p.login}`).join(', ')}.`,
    );
  }
  if (unlinked.length > 0) {
    lines.push(
      '',
      `These commits have an author not linked to a GitHub account: ${unlinked.join(', ')}. ` +
        'Write them with an e-mail address registered with your GitHub account and push again.',
    );
  }
  lines.push(
    '',
    'An acceptance is recorded with your GitHub account, the name and e-mail address of your ' +
      'commits, this pull request and the comment, in a private repository. Post `recheck` to ' +
      'check again.',
  );
  return lines.join('\n');
}

/** Create, update or leave the pull request's CLA comment. */
async function updateComment(github, context, number, body, needed) {
  const comments = await github.paginate(github.rest.issues.listComments, {
    ...context.repo,
    issue_number: number,
    per_page: 100,
  });
  const existing = comments.find((c) => String(c.body || '').startsWith(MARKER));
  if (existing) {
    if (existing.body !== body) {
      await github.rest.issues.updateComment({ ...context.repo, comment_id: existing.id, body });
    }
  } else if (needed) {
    await github.rest.issues.createComment({ ...context.repo, issue_number: number, body });
  }
}

/** The check. `token` is the CLA_SIGNATURES_TOKEN secret. */
module.exports = async function cla({ github, context, core, getOctokit, token }) {
  if (context.eventName === 'issue_comment') {
    const body = normalize(context.payload.comment.body);
    if (!context.payload.issue.pull_request || (body !== ACCEPTANCE && body !== RECHECK)) {
      return;
    }
  }
  const pr = await pullRequest(github, context);
  if (context.eventName === 'pull_request_target' && context.payload.action === 'closed') {
    if (pr.merged) {
      await github.rest.issues.lock({ ...context.repo, issue_number: pr.number, lock_reason: 'resolved' });
    }
    return;
  }
  const status = (state, description) =>
    github.rest.repos.createCommitStatus({
      ...context.repo,
      sha: pr.head.sha,
      state,
      context: STATUS_CONTEXT,
      description,
    });
  if (!token) {
    await status('error', 'The CLA check is not configured');
    core.setFailed('the CLA_SIGNATURES_TOKEN secret is missing');
    return;
  }
  const store = getOctokit(token);
  const { people, unlinked } = await authors(github, context, pr.number);
  const { content } = await readStore(store);
  const signed = new Set(content.signatures.map((s) => s.id));
  if (context.eventName === 'issue_comment') {
    const comment = context.payload.comment;
    const person = people.get(comment.user.id);
    if (normalize(comment.body) === ACCEPTANCE && person && !signed.has(person.id)) {
      const { data: agreement } = await github.rest.repos.getContent({
        ...context.repo,
        path: 'CLA.md',
        ref: pr.base.sha,
      });
      await addSignature(store, {
        login: person.login,
        id: person.id,
        name: person.name,
        email: person.email,
        cla_version: CLA_VERSION,
        cla_blob: agreement.sha,
        repository: `${context.repo.owner}/${context.repo.repo}`,
        pull_request: pr.number,
        comment: comment.html_url,
        accepted_at: comment.created_at,
      });
      signed.add(person.id);
    }
  }
  const pending = [...people.values()].filter((p) => !exempt(p.login) && !signed.has(p.id));
  const ok = pending.length === 0 && unlinked.length === 0;
  const link = `${pr.base.repo.html_url}/blob/${pr.base.repo.default_branch}/CLA.md`;
  await updateComment(github, context, pr.number, message(link, pending, unlinked), !ok);
  await status(
    ok ? 'success' : 'failure',
    ok ? 'Every author accepted the CLA' : 'An author has not accepted the CLA yet',
  );
  if (!ok) {
    core.info(`still to accept: ${pending.map((p) => p.login).join(', ') || '-'}; ` +
      `unlinked commits: ${unlinked.join(', ') || '-'}`);
  }
};

module.exports.ACCEPTANCE = ACCEPTANCE;
module.exports.MARKER = MARKER;
module.exports.SIGNATURES = SIGNATURES;
