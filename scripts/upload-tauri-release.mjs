import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = process.cwd();
const NSIS_DIR = path.join(ROOT, "src-tauri", "target", "release", "bundle", "nsis");
const DEFAULT_UPLOAD_URL = "https://release.zapwize.com/api/releases/sage-data-bridge";
const TOKEN = "xau835FmzV9z3ymQotLbp2krIk2Jv28o9Ok";

function requireEnv(name, fallback = "") {
  const value = process.env[name] || fallback;
  if (!value) throw new Error(`Missing required environment variable: ${name}`);
  return value;
}

function getArg(name, fallback = "") {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

async function loadPackage() {
  const raw = await readFile(path.join(ROOT, "package.json"), "utf8");
  return JSON.parse(raw);
}

function findArtifact(version, suffix) {
  const candidates = [
    `Sage Data Bridge_${version}_x64-setup${suffix}`,
    `Sage Data Bridge_${version}_x64_en-US-setup${suffix}`,
    `Sage Data Bridge_${version}_x64-setup.nsis${suffix === ".zip" ? ".zip" : suffix}`,
  ];

  for (const candidate of candidates) {
    const filePath = path.join(NSIS_DIR, candidate);
    if (existsSync(filePath)) return filePath;
  }

  throw new Error(`Could not find NSIS artifact ending in ${suffix} for version ${version} in ${NSIS_DIR}`);
}

async function appendFile(form, field, filePath, type) {
  const buffer = await readFile(filePath);
  const blob = new Blob([buffer], { type });
  form.append(field, blob, path.basename(filePath));
}

async function main() {
  const pkg = await loadPackage();
  const version = getArg("version", process.env.RELEASE_VERSION || pkg.version);
  const notes = getArg("notes", process.env.RELEASE_NOTES || `Sage Data Bridge ${version}`);
  const uploadUrl = requireEnv("RELEASE_UPLOAD_URL", DEFAULT_UPLOAD_URL);
  const token = requireEnv("RELEASE_UPLOAD_TOKEN", TOKEN);

  const exePath = findArtifact(version, ".exe");
  const zipPath = findArtifact(version, ".nsis.zip");
  const sigPath = `${zipPath}.sig`;

  if (!existsSync(sigPath)) {
    throw new Error(`Missing updater signature file: ${sigPath}`);
  }

  const signature = (await readFile(sigPath, "utf8")).trim();
  if (!signature || signature.includes("<")) {
    throw new Error(`Invalid signature content in ${sigPath}`);
  }

  const form = new FormData();
  form.append("version", version);
  form.append("notes", notes);
  form.append("pub_date", new Date().toISOString());
  form.append("signature", signature);
  await appendFile(form, "installer", exePath, "application/vnd.microsoft.portable-executable");
  await appendFile(form, "updater", zipPath, "application/zip");

  const response = await fetch(uploadUrl, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
    },
    body: form,
  });

  const body = await response.text();
  if (!response.ok) {
    throw new Error(`Release upload failed: HTTP ${response.status}\n${body}`);
  }

  console.log(body);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
