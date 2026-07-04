import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const ROOT = process.cwd();
const bump = process.argv[2];

if (!bump) {
  fail("Usage: npm run release:version -- <patch|minor|major|x.y.z>");
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function parseVersion(version) {
  const match = String(version).trim().match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match) fail(`Invalid semver version: ${version}`);
  return match.slice(1).map(Number);
}

function nextVersion(current, mode) {
  if (/^\d+\.\d+\.\d+$/.test(mode)) return mode;

  const [major, minor, patch] = parseVersion(current);
  switch (mode) {
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "major":
      return `${major + 1}.0.0`;
    default:
      fail(`Unknown version bump "${mode}". Use patch, minor, major, or x.y.z.`);
  }
}

async function readJson(relativePath) {
  const filePath = path.join(ROOT, relativePath);
  return JSON.parse(await readFile(filePath, "utf8"));
}

async function writeJson(relativePath, value) {
  const filePath = path.join(ROOT, relativePath);
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

async function replaceVersion(relativePath, pattern, replacement) {
  const filePath = path.join(ROOT, relativePath);
  const current = await readFile(filePath, "utf8");
  const next = current.replace(pattern, replacement);
  if (next === current) fail(`Could not update version in ${relativePath}`);
  await writeFile(filePath, next);
}

const packageJson = await readJson("package.json");
const version = nextVersion(packageJson.version, bump);

packageJson.version = version;
await writeJson("package.json", packageJson);

const packageLock = await readJson("package-lock.json");
packageLock.version = version;
if (packageLock.packages?.[""]) packageLock.packages[""].version = version;
await writeJson("package-lock.json", packageLock);

await replaceVersion(
  path.join("src-tauri", "Cargo.toml"),
  /^version = ".*"$/m,
  `version = "${version}"`,
);

const tauriConf = await readJson(path.join("src-tauri", "tauri.conf.json"));
tauriConf.package.version = version;
await writeJson(path.join("src-tauri", "tauri.conf.json"), tauriConf);

console.log(`Version updated to ${version}`);
