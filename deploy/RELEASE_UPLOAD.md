# Sage Data Bridge release upload

## Required GitHub secrets

- `TAURI_PRIVATE_KEY`: contents of the generated Tauri updater private key, or a path available on the runner.
- `TAURI_KEY_PASSWORD`: updater key password, currently `Louis14@`.
- `RELEASE_UPLOAD_URL`: `https://release.zapwize.com/api/releases/sage-data-bridge`
- `RELEASE_UPLOAD_TOKEN`: strong shared secret also configured in the Total.js app environment.

## Required Total.js setup

1. Copy `deploy/totaljs-release-controller.js` into the Total.js v4 app `controllers` folder.
2. Set `RELEASE_UPLOAD_TOKEN` in the Total.js app environment.
3. Ensure the app can write to `public/sage-data-bridge`.

## Release flow

1. Bump both app versions to the same semver:
   - `package.json`
   - `src-tauri/tauri.conf.json` under `package.version`
2. Run the GitHub Actions workflow: `Build and upload Windows release`.
3. The workflow builds Tauri, finds the NSIS artifacts, and uploads:
   - normal `.exe` installer
   - updater `.nsis.zip`
   - `.nsis.zip.sig` content inside `latest.json`

The Total.js route writes:

- `/public/sage-data-bridge/latest.json`
- `/public/sage-data-bridge/releases/vX.Y.Z/<installer.exe>`
- `/public/sage-data-bridge/releases/vX.Y.Z/<updater.nsis.zip>`

The manifest URL used by the Tauri app is:

`https://release.zapwize.com/sage-data-bridge/latest.json`
