# Tmux Pane Self-Hosting Design

## Context

Currently the daemon spawns agents as tmux **windows** (tabs) in a detached session, requiring the user to manually `just attach` to find them. This design changes the daemon to self-host inside tmux: it creates its own session, runs in the first pane, and spawns agents as additional **panes** in a tiled grid layout — similar to opencode.ai's sub-agent panel approach.

## Design

### 1. Auto Re-exec Mechanism (`src/main.rs`)

After CLI parsing and config loading, but before running the pipeline:

1. **Skip for `mark-done` subcommand** — it runs inside agent panes, no tmux logic needed.
2. **Check `$TMUX` env var**:
   - **Set** → already inside tmux. Run pipeline normally.
   - **Not set** → self-host:
     a. Check if session `config.tmux.session_name` already exists (`tmux has-session`).
     b. If session exists → just attach to it (handles crash recovery / re-attach).
     c. If no session → create detached session running this binary with the same args:
        ```
        tmux new-session -d -s <session_name> -n daemon <current_exe> <args...>
        ```
     d. Attach to the session (blocks until user detaches or session ends):
        ```
        tmux attach -t <session_name>
        ```
     e. Exit the outer process.

The outer process is a thin launcher. The inner instance (which sees `$TMUX`) runs the actual sync + spawner loops.

### 2. Pane-based Agent Spawning (`src/spawner.rs`)

Replace the current `new-window` / `new-session` logic in `spawn_tmux_session()`:

- **Remove** the `session_exists` check and `new-session` branch — the session always exists because the re-exec creates it.
- **Replace `new-window`** with `split-window`:
  ```
  tmux split-window -t <session_name> <script_path>
  ```
  Environment variables are passed the same way (via `.env(k, v)` on the Command).
- **Rebalance layout** after each split:
  ```
  tmux select-layout -t <session_name> tiled
  ```
  The `tiled` layout auto-arranges panes in a grid that rebalances as panes are added/removed.

### 3. Wrapper Script Pane Auto-close (`src/spawner.rs`)

Replace the current "Press Enter to close" logic in the generated wrapper script:

```bash
# Old:
echo "Session ended. Press Enter to close this pane."
read -r

# New:
echo ""
echo "Closing in 10 seconds..."
sleep 10
```

When the script exits after the delay, the pane closes and tmux rebalances the remaining panes.

### 4. Configuration

No new config fields. Uses existing `config.tmux.session_name` throughout. The pane behavior is automatic — the re-exec mechanism ensures the daemon always runs inside tmux.

### 5. `just attach` Compatibility

`just attach` continues to work unchanged — it reads `session_name` from config and runs `tmux attach -t <name>`. Useful for re-attaching after detach.

## Files to Modify

- `src/main.rs` — Add re-exec logic after config load, before pipeline start
- `src/spawner.rs` — Replace `new-window`/`new-session` with `split-window` + `select-layout tiled`, update wrapper script template

## Edge Cases

- **No tmux installed**: The re-exec fails with a clear error from `Command::new("tmux")`
- **Session name collision**: If session already exists (e.g., previous crash), attach to it instead of creating a new one
- **User launches inside tmux manually**: `$TMUX` is set, so re-exec is skipped and pane spawning works directly
- **Terminal too small**: tmux's `tiled` layout handles small terminals gracefully, stacking panes

## Verification

1. `just build` compiles without errors
2. `just check` passes (clippy + fmt)
3. `just test` — existing tests still pass
4. Manual test: run the daemon from a non-tmux terminal → it should create a tmux session and attach
5. Manual test: verify spawned agents appear as panes in a tiled grid
6. Manual test: detach (`Ctrl-B d`), then `just attach` reconnects
7. Manual test: verify pane auto-closes ~10s after agent finishes
