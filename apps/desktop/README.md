# DeepSeek Harness Desktop

Tauri 2 shell that spawns `dsh web --port 0 --host 127.0.0.1`, waits for the ready line, and loads the existing Web GUI.

Development (from this directory):

```sh
cd src-tauri
cargo test
cargo run
```

Packaging, data-directory, and token notes land in a later stage.
