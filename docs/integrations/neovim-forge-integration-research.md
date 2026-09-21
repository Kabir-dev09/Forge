# FORGE + NEOVIM INTEGRATION: RESEARCH & ARCHITECTURE

## 1. Feature Definition
The goal of the `forge-integrations` plugin is to seamlessly integrate Neovim's internal buffers with Forge's native tab system. When a user opens, renames, or deletes file buffers in Neovim, Forge dynamically creates, updates, or removes corresponding top-level Forge tabs. The Forge tab acts purely as a remote control surface and metadata representation for the Neovim buffer—it does not hold file content, and clicking it merely instructs Neovim to switch its active buffer, rather than spawning a duplicate editor or terminal. 

## 2. Current Forge Architecture
Based on an inspection of `crates/forge-main/src/mux/`, the current architecture is structured as follows:
- **`TabManager`**: Maintains a flat `Vec<Tab>` and tracks the `active_tab_index`.
- **`Tab`**: Uniquely identified by `TabId`. Currently, a `Tab` strictly owns a `MuxState`.
- **`MuxState`**: Manages the layout of `Pane`s (terminals).
- **`Pane`**: Owns a child process (`Option<Pty>`) and a `RenderSnapshot`.

**Architectural Friction:** Currently, 1 Tab = 1 Layout = 1+ Processes. To map Neovim buffers to Forge tabs, multiple Forge tabs will need to point to the *same* underlying Neovim `Pane`/`Pty`. 

## 3. Neovim Architecture Relevant to Integration
- **Buffer vs Window vs Tabpage**: 
  - A **Buffer** is the in-memory text of a file. (This is what we want to map to a Forge tab).
  - A **Window** is a viewport displaying a buffer.
  - A **Tabpage** is a layout of windows.
- **RPC System**: Neovim natively acts as a msgpack-RPC server (accessible via `v:servername`). It allows external clients to call API functions (e.g., `nvim_set_current_buf`) and allows plugins to push notifications to external channels (`rpcnotify`).
- **Autocommands**: Neovim provides event hooks (`BufAdd`, `BufDelete`, `BufEnter`, `BufFilePost`) which allow Lua callbacks to run immediately upon buffer lifecycle events.

## 4. Architecture Options

### Tab Model
*   **Model A - Integration Tab Type**: Introduce an `enum TabContent { Owned(MuxState), Proxy { target_pane: PaneId, buffer_id: u32 } }`. Proxy tabs render the snapshot of their `target_pane` but act as independent tabs in the UI.
*   **Model B - Generic Target**: Re-architect the entire Tab system so it has no terminal-specific concepts, only UI state.
*   **Model C - Augmented Terminal Tab**: Keep one Terminal Tab, but the UI draws "sub-tabs" for Neovim buffers when the terminal enters a specific mode.

### Communication Transport
*   **Option A - Neovim Native msgpack-RPC**: Forge connects to Neovim's `v:servername`.
*   **Option B - Forge Unix Socket**: Forge hosts an IPC server (`/tmp/forge-ipc.sock`); the Neovim plugin connects as a client.
*   **Option D - `nvim_ui_attach`**: Overkill; embedding the full UI rendering loop violates the "lightweight control surface" requirement.
*   **Option E/F - Filesystem/CLI**: Too high latency, too high CPU overhead.

## 5. Recommended Architecture
- **Tab Model**: **Model A (Integration Tab Type via Proxy)**. It requires the least invasive change to Forge while allowing buffer tabs to coexist gracefully with terminal tabs in the top-level UI.
- **Communication**: **Option B (Forge Unix Socket)** combined with Msgpack payload serialization.

## 6. Why This Architecture
*   **Latency & CPU (Very High)**: Unix sockets combined with event-driven msgpack RPC provide sub-millisecond latency with near-zero idle CPU usage. No polling loops are needed.
*   **Memory (Very High)**: We transmit metadata only (IDs, file paths), keeping memory overhead negligible.
*   **Reliability & Security (High)**: A local Unix Domain Socket (0600 permissions) ensures that only the local user can interact with Forge, immune to TCP port collision or unauthorized network access.
*   **Discovery (High)**: By injecting `$FORGE_IPC_SOCKET` into the PTY environment, the Neovim plugin knows exactly where to connect without guessing or port-scanning. This cleanly solves the discovery problem.
*   **Compatibility**: Proxy tabs naturally adapt to Forge's current `TabManager` without breaking existing split panes or terminal ownership.

## 7. Communication Protocol
A versioned, asynchronous JSON or Msgpack-over-socket protocol:
**Neovim → Forge (Events):**
*   `forge/handshake` (instance_id, capabilities)
*   `forge/buffer_added` (buffer_id, path, name)
*   `forge/buffer_removed` (buffer_id)
*   `forge/buffer_entered` (buffer_id)
*   `forge/buffer_renamed` (buffer_id, new_name)

**Forge → Neovim (Commands):**
*   `nvim/focus_buffer` (buffer_id)

## 8. `forge-integrations` Plugin Architecture
The Neovim plugin will be designed as an extensible platform:
```text
lua/forge-integrations/
├── init.lua       -- Setup function, capability negotiation
├── connection.lua -- Unix socket management, reconnect logic (uv.pipe)
├── buffers.lua    -- Tracks listed buffers, filters unlisted/terminals
└── events.lua     -- Registers autocmds (BufAdd, BufEnter, etc.)
```

## 9. Forge Architecture Changes
1. **IPC Server**: Add an async Unix socket server (e.g., using `tokio` or `calloop` depending on Forge's runtime) to listen for plugin connections.
2. **Tab Model**: Update `Tab` to support `Proxy` variants that point to a `PaneId` and carry an `instance_id` and `buffer_id`.
3. **Renderer**: Teach the renderer to resolve `Proxy` tabs to their target `Pane` for `RenderSnapshot` acquisition.
4. **Environment Injection**: Modify `forge_pty` to inject `FORGE_IPC_SOCKET` and `FORGE_PANE_ID` into the terminal environment.

## 10. Buffer Lifecycle
1. **Creation**: User runs `:e file.rs` -> Neovim fires `BufAdd` -> `events.lua` filters out unlisted buffers -> sends `forge/buffer_added` -> Forge creates a Proxy Tab.
2. **Focus**: User clicks the Proxy Tab -> Forge sends `nvim/focus_buffer(id)` -> Plugin receives it and calls `vim.api.nvim_set_current_buf(id)`.
3. **Rename**: User runs `:saveas new.rs` -> Neovim fires `BufFilePost` -> Plugin sends `forge/buffer_renamed` -> Forge updates Tab title.
4. **Deletion**: User runs `:bd` -> Neovim fires `BufDelete` -> Plugin sends `forge/buffer_removed` -> Forge removes the Tab.

## 11. Connection Lifecycle
*   **Discover**: Forge sets `FORGE_IPC_SOCKET=/tmp/forge-1234.sock` in the PTY.
*   **Connect**: Plugin uses `vim.uv.new_pipe(false)` to connect.
*   **Sync**: Upon connection, the plugin immediately iterates `vim.api.nvim_list_bufs()` and sends a bulk `forge/sync_state` to populate initial tabs.
*   **Disconnect**: If the socket drops, Forge removes all Proxy tabs associated with that instance and un-hides the base Terminal tab. The plugin enters a backoff-retry loop to reconnect.

## 12. Multi-Instance Model
Each Terminal `Pane` has a unique `PaneId`. This `PaneId` is passed in the environment as `FORGE_PANE_ID`. When the Neovim plugin handshakes, it sends this ID. Forge uses it to route tab clicks back to the correct socket and correctly render the parent terminal for the proxy tabs.

## 13. Tab Model
When a Neovim instance connects and sends its buffers, Forge hides the parent Terminal tab from the tab bar (but keeps the `MuxState` alive) and spawns the Proxy tabs. To the user, the "Terminal" morphs into a set of "Editor Buffers".

## 14. Error Handling
- **Invalid Buffer**: If Forge sends `focus_buffer` for a deleted buffer (race condition), the plugin safely catches the error via `pcall` and replies with a `buffer_removed` correction event.
- **RPC Desync**: If message parsing fails, the connection is dropped and cleanly re-established, triggering a full state sync.

## 15. Security
The IPC Unix socket is created in `$XDG_RUNTIME_DIR` (e.g., `/run/user/1000/forge/`) with `0600` permissions. Only the local user can connect. No TCP ports are exposed.

## 16. Performance
- **Idle**: Zero CPU. The plugin relies entirely on native C-level `autocmds` which do nothing unless a buffer changes.
- **Latency**: Unix socket IPC has sub-millisecond overhead, meaning tab clicks will feel instantaneous. No polling is utilized anywhere in the stack.

## 17. Testing Strategy
- **Plugin Unit Tests**: Use `plenary.nvim` to test connection resilience and event debouncing without a real Forge server.
- **Forge IPC Tests**: Test the IPC listener by sending mocked JSON/Msgpack payloads to verify Tab creation/deletion logic.
- **E2E**: Spawn Forge, type `nvim test.txt`, and verify the tab appears via accessibility/UI automation tools.

## 18. Implementation Plan
1. **Forge IPC Core**: Implement the Unix socket server in Forge and environment variable injection.
2. **Plugin Core**: Scaffold `forge-integrations`, implement `uv.pipe` connection and JSON/Msgpack serialization.
3. **Tab Abstraction**: Refactor `Tab` in Forge to support Proxy variants and modify the renderer to resolve them.
4. **State Sync**: Implement the initial buffer sync handshake.
5. **Lifecycle Events**: Implement Lua autocommands for Add/Remove/Rename/Enter.
6. **Interaction**: Implement Forge tab click routing to `nvim_set_current_buf`.

## 19. Open Questions
- **Multiple Windows**: If one Neovim instance has multiple splits (windows) showing different buffers, clicking a Forge tab currently uses `nvim_set_current_buf()`, which changes the buffer of the *currently active window*. If the user wants to jump to the window that is *already* showing that buffer, the plugin logic will need to scan `nvim_list_wins()` first. *Recommendation: Implement the simple current-window swap first, and refine based on UX feedback.*
- **Unlisted Buffers**: Should terminal buffers (`:term`) inside Neovim be represented as Forge tabs? *Recommendation: Exclude them initially by strictly checking `vim.bo.buflisted`.*
