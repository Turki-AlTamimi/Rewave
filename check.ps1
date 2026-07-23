# Stage 0 gate check (Windows). Full workspace including the Tauri shell.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
cargo build --workspace
npm run build --prefix rewave-ui
