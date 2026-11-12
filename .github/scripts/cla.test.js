// Tests of the CLA check against an in-memory GitHub: node --test .github/scripts/cla.test.js
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const cla = require('./cla.js');

const OWNER = { login: 'skullab', id: 1 };
const ALICE = { login: 'alice', id: 2 };
const BOB = { login: 'bob', id: 3 };
const BOT = { login: 'dependabot[bot]', id: 4 };

/** A pull request, its commits and comments, and the private acceptances file. */
function world({ authors, comments = [], signatures = [], merged = false }) {
  const state = {
    comments: comments.map((body, i) => ({ id: 100 + i, body })),
    statuses: [],
    locked: false,
    file: signatures.length
      ? { agreement: 'x', version: 1, signatures: signatures.map((u) => ({ ...u })) }
      : null,
    writes: 0,
  };
  const pr = {
    number: 7,
    merged,
    head: { sha: 'head-sha' },
    base: {
      sha: 'base-sha',
      repo: { html_url: 'https://github.com/dazeroit/karasu-slicer', default_branch: 'main' },
    },
  };
  const commits = authors.map((user, i) => ({
    sha: `c0ffee${i}abcdef`,
    author: user,
    commit: { author: { name: user ? `${user.login} name` : 'Nobody', email: 'x@example.org' } },
  }));
  const notFound = () => Object.assign(new Error('not found'), { status: 404 });
  const github = {
    paginate: async (method, params) => (await method(params)).data,
    rest: {
      pulls: {
        get: async () => ({ data: pr }),
        listCommits: async () => ({ data: commits }),
      },
      issues: {
        listComments: async () => ({ data: state.comments }),
        createComment: async ({ body }) => {
          state.comments.push({ id: 900, body });
        },
        updateComment: async ({ comment_id: id, body }) => {
          state.comments.find((c) => c.id === id).body = body;
        },
        lock: async () => {
          state.locked = true;
        },
      },
      repos: {
        createCommitStatus: async (status) => {
          state.statuses.push(status);
        },
        getContent: async ({ path }) => {
          assert.equal(path, 'CLA.md');
          return { data: { sha: 'cla-blob' } };
        },
      },
    },
  };
  const store = {
    rest: {
      repos: {
        getContent: async ({ owner, repo, path }) => {
          assert.deepEqual([owner, repo, path], ['dazeroit', 'karasu-slicer-cla', 'signatures/cla-v1.json']);
          if (!state.file) {
            throw notFound();
          }
          return {
            data: { sha: 'file-sha', content: Buffer.from(JSON.stringify(state.file)).toString('base64') },
          };
        },
        createOrUpdateFileContents: async ({ content }) => {
          state.file = JSON.parse(Buffer.from(content, 'base64').toString('utf8'));
          state.writes += 1;
        },
      },
    },
  };
  const core = { failed: null, setFailed: (m) => { core.failed = m; }, info: () => {} };
  return { state, pr, github, store, core };
}

function run(w, event) {
  const context = {
    repo: { owner: 'dazeroit', repo: 'karasu-slicer' },
    eventName: event.name,
    payload: event.payload,
  };
  return cla({
    github: w.github,
    context,
    core: w.core,
    getOctokit: () => w.store,
    token: event.token === undefined ? 'token' : event.token,
  });
}

const opened = (w) => ({ name: 'pull_request_target', payload: { action: 'opened', pull_request: w.pr } });
const comment = (user, body) => ({
  name: 'issue_comment',
  payload: {
    issue: { number: 7, pull_request: {} },
    comment: { user, body, html_url: 'https://github.com/c/1', created_at: '2026-11-12T10:00:00Z' },
  },
});

test('an author who has not accepted fails the check and is asked to', async () => {
  const w = world({ authors: [ALICE] });
  await run(w, opened(w));
  assert.equal(w.state.statuses.at(-1).state, 'failure');
  assert.equal(w.state.statuses.at(-1).sha, 'head-sha');
  const body = w.state.comments[0].body;
  assert.ok(body.startsWith(cla.MARKER));
  assert.ok(body.includes('@alice'));
  assert.ok(body.includes(cla.ACCEPTANCE));
  assert.ok(body.includes('https://github.com/dazeroit/karasu-slicer/blob/main/CLA.md'));
});

test('the acceptance comment records the author and passes the check', async () => {
  const w = world({ authors: [ALICE, ALICE] });
  await run(w, opened(w));
  await run(w, comment(ALICE, `  ${cla.ACCEPTANCE.replace(/ /g, '\n')}  `));
  assert.equal(w.state.writes, 1);
  const [entry] = w.state.file.signatures;
  assert.equal(entry.login, 'alice');
  assert.equal(entry.id, 2);
  assert.equal(entry.name, 'alice name');
  assert.equal(entry.cla_version, 1);
  assert.equal(entry.cla_blob, 'cla-blob');
  assert.equal(entry.pull_request, 7);
  assert.equal(entry.accepted_at, '2026-11-12T10:00:00Z');
  assert.equal(w.state.statuses.at(-1).state, 'success');
  assert.equal(w.state.comments.length, 1);
  assert.ok(w.state.comments[0].body.includes('Every author'));
  // Accepting again changes nothing.
  await run(w, comment(ALICE, cla.ACCEPTANCE));
  assert.equal(w.state.writes, 1);
});

test('only the authors of the commits can accept, and only with the exact sentence', async () => {
  const w = world({ authors: [ALICE] });
  // Someone else's acceptance records nothing and the check stays failed.
  await run(w, comment(BOB, cla.ACCEPTANCE));
  assert.equal(w.state.writes, 0);
  assert.equal(w.state.statuses.at(-1).state, 'failure');
  // Another sentence is not an acceptance and does not even run the check.
  const before = w.state.statuses.length;
  await run(w, comment(ALICE, 'I agree to the Contributor Licence Agreement'));
  assert.equal(w.state.writes, 0);
  assert.equal(w.state.statuses.length, before);
});

test('every author must accept, each recorded once', async () => {
  const w = world({ authors: [ALICE, BOB], signatures: [{ login: 'alice', id: 2 }] });
  await run(w, opened(w));
  assert.equal(w.state.statuses.at(-1).state, 'failure');
  assert.ok(w.state.comments[0].body.includes('@bob'));
  assert.ok(!w.state.comments[0].body.includes('@alice'));
  await run(w, comment(BOB, cla.ACCEPTANCE));
  assert.equal(w.state.file.signatures.length, 2);
  assert.equal(w.state.statuses.at(-1).state, 'success');
});

test('the owner and bots need no acceptance, and no comment is posted', async () => {
  const w = world({ authors: [OWNER, BOT] });
  await run(w, opened(w));
  assert.equal(w.state.statuses.at(-1).state, 'success');
  assert.deepEqual(w.state.comments, []);
});

test('a commit whose author has no GitHub account fails the check', async () => {
  const w = world({ authors: [OWNER, null] });
  await run(w, opened(w));
  assert.equal(w.state.statuses.at(-1).state, 'failure');
  assert.ok(w.state.comments[0].body.includes('c0ffee1'));
});

test('recheck evaluates again without recording anything', async () => {
  const w = world({ authors: [ALICE], signatures: [{ login: 'alice', id: 2 }] });
  await run(w, comment(BOB, 'recheck'));
  assert.equal(w.state.statuses.at(-1).state, 'success');
  assert.equal(w.state.writes, 0);
});

test('a merged pull request is locked; a closed one is not', async () => {
  const merged = world({ authors: [ALICE], merged: true });
  await run(merged, { name: 'pull_request_target', payload: { action: 'closed', pull_request: merged.pr } });
  assert.equal(merged.state.locked, true);
  const closed = world({ authors: [ALICE] });
  await run(closed, { name: 'pull_request_target', payload: { action: 'closed', pull_request: closed.pr } });
  assert.equal(closed.state.locked, false);
});

test('a missing token reports an error instead of passing', async () => {
  const w = world({ authors: [ALICE] });
  await run(w, { ...opened(w), token: '' });
  assert.equal(w.state.statuses.at(-1).state, 'error');
  assert.match(w.core.failed, /CLA_SIGNATURES_TOKEN/);
});

test('comments on issues, and other comments, are ignored', async () => {
  const w = world({ authors: [ALICE] });
  await run(w, { name: 'issue_comment', payload: { issue: { number: 7 }, comment: { user: ALICE, body: cla.ACCEPTANCE } } });
  await run(w, comment(ALICE, 'Looks good'));
  assert.deepEqual(w.state.statuses, []);
  assert.equal(w.state.writes, 0);
});
