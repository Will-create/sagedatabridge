@echo off
setlocal

set "ROOT=%~dp0"
set "TAURI_PRIVATE_KEY=%ROOT%.tauri\sage-data-bridge.key"
set "TAURI_KEY_PASSWORD=Louis14@"

if not exist "%TAURI_PRIVATE_KEY%" (
  echo Missing Tauri private key:
  echo %TAURI_PRIVATE_KEY%
  exit /b 1
)

call npm run tauri -- build
exit /b %ERRORLEVEL%
