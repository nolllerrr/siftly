import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
const lock = readFileSync(new URL('../src-tauri/Cargo.lock', import.meta.url), 'utf8');
const cargoVersion = cargo.split('[package]')[1]?.split(/\r?\n\[/)[0]?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const lockVersion = lock.split('[[package]]').find(block => /^name = "siftly"$/m.test(block))?.match(/^version = "([^"]+)"/m)?.[1];
const tag = process.env.RELEASE_TAG;
assert.match(tag ?? '', /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/, 'RELEASE_TAG must be a stable version such as v0.1.0');
for (const [name, version] of Object.entries({ package: pkg.version, tauri: config.version, cargo: cargoVersion, lock: lockVersion })) {
  assert.equal(`v${version}`, tag, `${name} version does not match tag`);
}
console.log(`Release versions verified: ${tag}`);
