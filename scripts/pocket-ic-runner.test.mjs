import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  copyFileSync,
  readFileSync,
  existsSync,
  rmSync,
  realpathSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

function run(
  t,
  { build = 0, tests = 0, fetch = 0, locked = false, emptyServer = false, sameTarget = false } = {},
) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'private-perp-runner-')))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  mkdirSync(join(root, 'scripts'))
  mkdirSync(join(root, 'bin'))
  copyFileSync(
    new URL('./pocket-ic-test.sh', import.meta.url),
    join(root, 'scripts/pocket-ic-test.sh'),
  )
  writeFileSync(
    join(root, 'scripts/fetch-pocket-ic.sh'),
    `${emptyServer ? '' : 'echo /fixture/pocket-ic'}\nexit ${fetch}\n`,
  )
  writeFileSync(
    join(root, 'bin/cargo'),
    `#!/bin/bash
echo "$*" >> "$TEST_COMMAND_LOG"
echo "\${CARGO_TARGET_DIR:-unset}" >> "$TEST_TARGET_LOG"
if [[ "$1" == build ]]; then exit ${build}; fi
echo 'test result: ok. 1 passed; 0 failed; 0 ignored'
exit ${tests}
`,
    { mode: 0o700 },
  )
  // 待機時間だけを試験側で短縮する。本番の試行回数は変えない。
  writeFileSync(join(root, 'bin/sleep'), '#!/bin/bash\nexit 0\n', { mode: 0o700 })
  const lock = join(root, 'target/pocket-ic-test.lock.d')
  if (locked) mkdirSync(lock, { recursive: true })
  const log = join(root, 'commands.log')
  const targets = join(root, 'targets.log')
  const result = spawnSync('bash', [join(root, 'scripts/pocket-ic-test.sh'), '--no-fail-fast'], {
    encoding: 'utf8',
    timeout: 15000,
    env: {
      ...process.env,
      PATH: `${join(root, 'bin')}:${process.env.PATH}`,
      TEST_COMMAND_LOG: log,
      TEST_TARGET_LOG: targets,
      CARGO_TARGET_DIR: join(root, 'inherited-target'),
      POCKET_IC_TEST_DIR: join(root, sameTarget ? 'target' : 'target/test-venue'),
      POCKET_IC_BIN_OVERRIDE: '',
    },
  })
  if (result.error) throw result.error
  return {
    ...result,
    root,
    lockExists: existsSync(lock),
    commands: existsSync(log) ? readFileSync(log, 'utf8') : '',
    targets: existsSync(targets) ? readFileSync(targets, 'utf8').trim().split('\n') : [],
  }
}

test('success builds isolated wasm and runs tests before announcing completion', (t) => {
  const result = run(t)
  assert.equal(result.status, 0)
  assert.equal(result.commands.split('\n').filter(Boolean).length, 6)
  assert.equal(result.commands.split('\n').filter((line) => line.includes('-p private-perp')).length, 2)
  assert.match(result.commands, /test --locked -p pocket-ic-tests --no-fail-fast/)
  assert.match(result.stdout, /all requested tests completed successfully/)
  assert.equal(result.lockExists, false)
  assert.deepEqual(result.targets, [
    join(result.root, 'target'),
    join(result.root, 'target'),
    join(result.root, 'target/test-venue'),
    join(result.root, 'target/test-venue'),
    join(result.root, 'target/test-venue'),
    join(result.root, 'inherited-target'),
  ])
})
for (const [name, options, status, count] of [
  ['build failure', { build: 23 }, 23, 1],
  ['test failure', { tests: 24 }, 24, 6],
  ['server failure', { fetch: 25 }, 25, 0],
  ['lock failure', { locked: true }, 1, 0],
  ['empty server path', { emptyServer: true }, 1, 0],
  ['overlapping wasm targets', { sameTarget: true }, 1, 0],
])
  test(name, (t) => {
    const result = run(t, options)
    assert.equal(result.status, status)
    assert.equal(result.commands.split('\n').filter(Boolean).length, count)
    assert.doesNotMatch(result.stdout, /all requested tests completed successfully/)
    assert.equal(result.lockExists, options.locked ?? false)
  })
