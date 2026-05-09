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
# sagedatabridge
