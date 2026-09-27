# AREITU

## Overview
TBD

## Setup
TBD

## Development
Rust stable が必要です。

```bash
cargo test --workspace
cargo run -p areitu-cli -- ingest-photos <写真フォルダ>
cargo run -p areitu-cli -- ingest-calendar <events.json>
cargo run -p areitu-cli -- build [--ollama-model <モデル名>]
cargo run -p areitu-cli -- list --sort recent --search <キーワード>
```
