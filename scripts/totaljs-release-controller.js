// Total.js v4 controller template for Sage Data Bridge releases.
//
// Put this file in your Total.js app controllers folder, then set:
// RELEASE_UPLOAD_TOKEN=<strong secret token>
//
// The uploader POSTs multipart/form-data to:
// POST /api/releases/sage-data-bridge
//
// Public files written by this route:
// /public/sage-data-bridge/latest.json
// /public/sage-data-bridge/releases/vX.Y.Z/<installer.exe>
// /public/sage-data-bridge/releases/vX.Y.Z/<updater.nsis.zip>

const Fs = require("fs");
const Path = require("path");

const APP_SLUG = "sage-data-bridge";
const PUBLIC_ROOT = Path.join(process.cwd(), "public", APP_SLUG);

exports.install = function () {
  ROUTE("POST /api/releases/sage-data-bridge", upload_release, ["upload"], 2048);
};

async function upload_release($) {
  const expectedToken = process.env.RELEASE_UPLOAD_TOKEN;
  const authorization = $.headers.authorization || "";
  const token = authorization.startsWith("Bearer ") ? authorization.slice(7).trim() : "";

  if (!expectedToken || token !== expectedToken) {
    $.invalid(401);
    return;
  }

  const body = $.body || {};
  const version = clean_version(body.version);
  const notes = String(body.notes || "");
  const pubDate = body.pub_date ? new Date(body.pub_date) : new Date();
  const signature = String(body.signature || "").trim();

  if (!version || !signature) {
    $.invalid(400);
    return;
  }

  const files = $.files || [];
  const installer = files.find((file) => file.name === "installer");
  const updater = files.find((file) => file.name === "updater");

  if (!installer || !updater || !updater.filename.endsWith(".nsis.zip")) {
    $.invalid(400);
    return;
  }

  const releaseDir = Path.join(PUBLIC_ROOT, "releases", `v${version}`);
  Fs.mkdirSync(releaseDir, { recursive: true });

  const installerName = safe_filename(installer.filename);
  const updaterName = safe_filename(updater.filename);
  const installerPath = Path.join(releaseDir, installerName);
  const updaterPath = Path.join(releaseDir, updaterName);

  await move_uploaded_file(installer, installerPath);
  await move_uploaded_file(updater, updaterPath);

  const updaterUrl = `https://release.zapwize.com/${APP_SLUG}/releases/v${version}/${encodeURIComponent(updaterName)}`;
  const manifest = {
    version,
    notes,
    pub_date: pubDate.toISOString(),
    platforms: {
      "windows-x86_64": {
        signature,
        url: updaterUrl,
      },
    },
  };

  Fs.mkdirSync(PUBLIC_ROOT, { recursive: true });
  Fs.writeFileSync(Path.join(PUBLIC_ROOT, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

  $.json({
    success: true,
    latest: `/${APP_SLUG}/latest.json`,
    installer: `/${APP_SLUG}/releases/v${version}/${installerName}`,
    updater: `/${APP_SLUG}/releases/v${version}/${updaterName}`,
  });
}

function clean_version(value) {
  const version = String(value || "").replace(/^v/i, "").trim();
  return /^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$/.test(version) ? version : "";
}

function safe_filename(value) {
  return Path.basename(String(value || "")).replace(/[^\w .@()+-]/g, "_");
}

function move_uploaded_file(file, destination) {
  return new Promise((resolve, reject) => {
    if (typeof file.move === "function") {
      file.move(destination, (error) => (error ? reject(error) : resolve()));
      return;
    }

    const source = file.path || file.filename;
    if (!source || source === destination) {
      resolve();
      return;
    }

    Fs.copyFile(source, destination, (copyError) => {
      if (copyError) {
        reject(copyError);
        return;
      }
      Fs.unlink(source, () => resolve());
    });
  });
}
