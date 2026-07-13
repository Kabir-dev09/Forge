# Right-Click Context Menu Implementation Report

## Files Changed

- `crates/forge-main/src/context_menu.rs`
- `crates/forge-main/src/event_loop.rs`
- `crates/forge-main/src/main.rs`
- `crates/forge-main/src/wayland/connection.rs`
- `crates/forge-main/src/wayland/seat.rs`
- `crates/forge-renderer/src/grid_tessellator.rs`
- `crates/forge-renderer/src/renderer.rs`

## How It Works

Right-click opens a small context menu at the pointer position when terminal mouse tracking is not active. The menu currently contains `Copy` and `Paste`. Left-clicking a menu item runs its action and closes the menu. Clicking outside the menu closes it without changing terminal content.

`Copy` reads the current terminal selection and publishes it through the existing Wayland clipboard manager using the pointer-button serial from the opening click path. `Paste` reuses the existing clipboard request path, so pasted text still flows through the terminal's existing paste handling and bracketed-paste support.

The menu closes on pointer leave, scroll, and window resize to avoid stale overlay geometry.

## Resource Behavior

When closed, the menu state is `None`. No menu layout, hit-testing, rendering, vertex upload, polling, or background work happens in that state.

When opened or when the hovered item changes, the event loop marks one redraw. The renderer receives optional menu render data only while the menu is open. Overlay vertices are appended after terminal foreground vertices so the menu appears above terminal text.

The renderer's partial-upload state tracks a separate context-menu range and fingerprint, so menu hover/open/close changes upload correctly without disabling existing row-level upload optimization for normal frames.

## Limitations

- The menu is intentionally minimal and only supports `Copy` and `Paste`.
- The menu opens only when terminal mouse tracking is disabled. Applications such as `nvim`, `btop`, and `tmux` that enable mouse tracking continue to receive mouse events.
- Styling is intentionally simple.

## Future Improvements

- Add disabled visual state when no selection exists.
- Add more actions through the existing item/action model.
- Add keyboard navigation if needed.
