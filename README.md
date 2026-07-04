# Sage Data Bridge

A fast, lightweight data extraction and visualization tool specialized for Sage Accounting (SQL Server).

## Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 1.70+
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites)

## Setup

```bash
# Install Node dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Release, updater, and upload workflow

The app uses the Tauri v1 updater. The updater manifest is expected at:

```text
https://release.zapwize.com/sage-data-bridge/latest.json
```

The manifest must point to the generated `.nsis.zip` updater bundle, not the `.exe` installer. Tauri generates this file during a signed Windows build.

### Bump versions

Use the release version script so all version files stay aligned:

```bash
npm run release:version -- patch
npm run release:version -- minor
npm run release:version -- major
```

Or set an exact version:

```bash
npm run release:version -- 6.0.1
```

This updates:

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

The app version starts at `6.0.0`.

### Build a signed Windows release

Set the updater signing environment variables before building:

```powershell
$env:TAURI_PRIVATE_KEY="C:\Users\USER\Downloads\sage-data-bridge\.tauri\sage-data-bridge.key"
$env:TAURI_KEY_PASSWORD="Louis14@"

npm run tauri build
```

The Windows artifacts are generated under:

```text
src-tauri/target/release/bundle/nsis/
```

Important files:

- `Sage Data Bridge_<version>_x64-setup.exe`
- `Sage Data Bridge_<version>_x64-setup.nsis.zip`
- `Sage Data Bridge_<version>_x64-setup.nsis.zip.sig`

The updater server must use the `.nsis.zip` URL and the contents of `.nsis.zip.sig`.

### Upload a release manually

Set the release server environment variables:

```powershell
$env:RELEASE_UPLOAD_URL="https://release.zapwize.com/api/releases/sage-data-bridge"
$env:RELEASE_UPLOAD_TOKEN="<bearer-token>"

npm run release:upload
```

The upload script sends:

- normal `.exe` installer
- updater `.nsis.zip`
- `.nsis.zip.sig` content
- version, notes, and publish date

### GitHub Actions release

The workflow is defined at:

```text
.github/workflows/release-windows.yml
```

Required GitHub secrets:

- `TAURI_PRIVATE_KEY`
- `TAURI_KEY_PASSWORD`
- `RELEASE_UPLOAD_URL`
- `RELEASE_UPLOAD_TOKEN`

Run the workflow manually and provide the release version and notes. The workflow builds the app and uploads the updater artifacts automatically.

### Total.js v4 release server

The Total.js upload route template is available in both:

```text
deploy/totaljs-release-controller.js
scripts/totaljs-release-controller.js
```

Copy it into the Total.js app `controllers` folder and set:

```bash
RELEASE_UPLOAD_TOKEN=<same-token-used-by-the-uploader>
```

The route accepts:

```text
POST /api/releases/sage-data-bridge
Authorization: Bearer <token>
```

It writes public files to:

```text
public/sage-data-bridge/latest.json
public/sage-data-bridge/releases/vX.Y.Z/<installer.exe>
public/sage-data-bridge/releases/vX.Y.Z/<updater.nsis.zip>
```

Generated `latest.json` shape:

```json
{
  "version": "6.0.0",
  "notes": "Release notes",
  "pub_date": "2026-07-04T00:00:00.000Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of .nsis.zip.sig>",
      "url": "https://release.zapwize.com/sage-data-bridge/releases/v6.0.0/Sage%20Data%20Bridge_6.0.0_x64-setup.nsis.zip"
    }
  }
}
```

## Stack

- **Backend**: Rust + Tiberius (SQL Server driver)
- **Desktop**: Tauri v1
- **Frontend**: React + Vite

## Features

- Connect to multiple SQL Server databases
- Browse tables and schemas
- Filter data with column-based conditions
- Paginated data preview
- CSV / Excel / SQL export
- App password protection
- Offline capable
