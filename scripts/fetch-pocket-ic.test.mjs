import test from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { gzipSync } from 'node:zlib'

function fixture(t, expectedHash) {
  const root = mkdtempSync(join(tmpdir(), 'private-perp-fetch-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  mkdirSync(join(root, 'scripts'))
  mkdirSync(join(root, 'bin'))
  const script = join(root, 'scripts/fetch-pocket-ic.sh')
  copyFileSync(new URL('./fetch-pocket-ic.sh', import.meta.url), script)
  const binary = Buffer.from('#!/bin/bash\necho pocket-ic-server 16.0.0\n')
  const archive = join(root, 'fixture.gz')
  writeFileSync(archive, gzipSync(binary))
  const calls = join(root, 'curl-calls')
  writeFileSync(join(root, 'bin/curl'), `#!/bin/bash
printf 'download\\n' >> "$FETCH_CALL_LOG"
cp "$FETCH_FIXTURE_GZIP" "\${@: -1}"
`, { mode: 0o700 })
  const destination = join(root, 'download/pocket-ic')
  const run = () => spawnSync('bash', [script], {
    encoding: 'utf8',
    env: {
      ...process.env,
      PATH: `${join(root, 'bin')}:${process.env.PATH}`,
      POCKET_IC_DIR: join(root, 'download'),
      POCKET_IC_SHA256: expectedHash ?? createHash('sha256').update(binary).digest('hex'),
      POCKET_IC_SERVER_MAJOR: '16.',
      FETCH_FIXTURE_GZIP: archive,
      FETCH_CALL_LOG: calls,
    },
  })
  return { run, destination, calls }
}

test('fresh download and cached server return only the executable path on stdout', (t) => {
  const { run, destination, calls } = fixture(t)
  for (const expected of ['download', 'cached']) {
    const result = run()
    assert.equal(result.status, 0, result.stderr)
    assert.equal(result.stdout, `${destination}\n`)
    assert.match(result.stderr, new RegExp(expected === 'cached' ? 'cached' : 'https://'))
  }
  assert.equal(readFileSync(calls, 'utf8'), 'download\n')
})

test('a fresh server with the wrong checksum never returns a path', (t) => {
  const { run } = fixture(t, '0'.repeat(64))
  const result = run()
  assert.notEqual(result.status, 0)
  assert.equal(result.stdout, '')
})
