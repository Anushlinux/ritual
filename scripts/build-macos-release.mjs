#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bundleRoot = path.join(root, 'src-tauri', 'target', 'release', 'bundle');
const requiredEnv = [
  'APPLE_SIGNING_IDENTITY',
  'APPLE_ID',
  'APPLE_PASSWORD',
  'APPLE_TEAM_ID',
];

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    env: process.env,
    stdio: 'inherit',
    ...options,
  });

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function removeIfExists(target) {
  if (existsSync(target)) {
    rmSync(target, { recursive: true, force: true });
  }
}

function walkFiles(dir) {
  if (!existsSync(dir)) return [];
  const entries = readdirSync(dir).flatMap((entry) => {
    const fullPath = path.join(dir, entry);
    const stat = statSync(fullPath);
    return stat.isDirectory() ? walkFiles(fullPath) : [fullPath];
  });
  return entries;
}

if (process.platform !== 'darwin') {
  console.error('macOS release builds must run on macOS.');
  process.exit(1);
}

const missing = requiredEnv.filter((name) => !process.env[name]?.trim());
if (missing.length > 0) {
  console.error(`Missing required signing/notarization env vars: ${missing.join(', ')}`);
  console.error('Install a Developer ID Application certificate, then set APPLE_SIGNING_IDENTITY, APPLE_ID, APPLE_PASSWORD, and APPLE_TEAM_ID.');
  process.exit(1);
}

removeIfExists(path.join(bundleRoot, 'macos', 'Ritual.app'));
removeIfExists(path.join(bundleRoot, 'macos', 'imprint.app'));
removeIfExists(path.join(bundleRoot, 'dmg'));

run('npx', ['tauri', 'build', '--bundles', 'app,dmg']);

const appPath = path.join(bundleRoot, 'macos', 'Ritual.app');
const dmgPaths = walkFiles(path.join(bundleRoot, 'dmg')).filter((file) => file.endsWith('.dmg'));

if (!existsSync(appPath)) {
  console.error(`Expected app bundle was not created: ${appPath}`);
  process.exit(1);
}

if (dmgPaths.length === 0) {
  console.error(`Expected at least one DMG under ${path.join(bundleRoot, 'dmg')}`);
  process.exit(1);
}

run('codesign', ['--verify', '--deep', '--strict', '--verbose=4', appPath]);
run('spctl', ['--assess', '--type', 'execute', '--verbose=4', appPath]);
run('xcrun', ['stapler', 'validate', appPath]);

for (const dmgPath of dmgPaths) {
  run('xcrun', ['stapler', 'validate', dmgPath]);
}

console.log('\nRelease artifacts:');
console.log(`- ${appPath}`);
for (const dmgPath of dmgPaths) {
  console.log(`- ${dmgPath}`);
}
