# Blur Implementation Plan

## Goal

Add background blur support without hurting normal rendering performance or stability. The correct target is **compositor-native blur over a translucent Wayland surface**, not renderer-side blur. Forge should:

- Use native Wayland blur protocols when available.
- Keep `window.opacity` / background alpha working correctly.
- Fall back cleanly to transparency-only when blur is unavailable.
- Do no extra per-frame work when blur is disabled.
- Avoid compositor-specific hacks in renderer code.

## Current Codebase Findings

Forge is a Wayland-native Vulkan terminal.

Evidence:

- Wayland connection/globals live in [connection.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/wayland/connection.rs).
- Window creation uses `wl_surface + xdg_surface + xdg_toplevel` in [window.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/wayland/window.rs).
- Vulkan surface is created from raw Wayland display/surface pointers in [surface.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/surface.rs).
- Swapchain already chooses `PRE_MULTIPLIED` composite alpha when supported in [swapchain.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/swapchain.rs).
- Render pass clears the swapchain every rendered frame in [render_pass.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/render_pass.rs).
- Background clear alpha comes from `theme.background.a * window.opacity` in [event_loop.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/event_loop.rs).
- SHM first frame also includes alpha in [main.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/main.rs) and [shm_buffer.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/wayland/shm_buffer.rs).
- Config already has `window.opacity` but no blur config in [config_registry.rs](/home/kabir/PROJECTS/Forge/crates/forge-core/src/config_registry.rs).
- Lua config extraction already parses `window.opacity` in [extractor.rs](/home/kabir/PROJECTS/Forge/crates/forge-config/src/extractor.rs).
- `forge-main` already depends on `wayland-protocols-plasma`, which exposes KDE blur protocol bindings in [Cargo.toml](/home/kabir/PROJECTS/Forge/crates/forge-main/Cargo.toml).

Important implication: blur needs the surface to remain translucent. If alpha is effectively `1.0`, compositor blur may be enabled but invisible.

## Wayland Blur Background

Wayland blur is compositor-controlled. The app cannot generally sample and blur the desktop behind itself because Wayland intentionally isolates client buffers.

Relevant protocols and compositor behavior:

- `ext-background-effect-v1` is a staging Wayland protocol for effects like background blur. It exposes compositor capabilities, a background-effect object per `wl_surface`, and `set_blur_region`; the blur region is double-buffered and applied on the next `wl_surface.commit`.
- KDE/Plasma has its own `org_kde_kwin_blur` protocol. The local crate registry includes `wayland-protocols-plasma` with `blur.xml`, exposing `org_kde_kwin_blur_manager.create(surface)`, `set_region`, `commit`, and `unset`.
- Hyprland blur is primarily compositor configuration driven. Hyprland documents global blur settings like `decoration:blur:enabled`, `size`, `passes`, `ignore_opacity`, etc. and window rules such as `no_blur`, `opaque`, `force_rgbx`, `xray`, and opacity rules.
- Niri documents `background-effect { blur true }` for windows, with support noted since 26.04 and warns that non-xray blur is experimental with limitations.
- GNOME/Mutter does not expose a general app-controlled blur protocol for ordinary xdg-toplevel clients.
- Sway/river/wlroots compositors generally should be treated as “transparency only unless a specific compositor supports a blur protocol/rule.”

## Possible Approaches

1. **Renderer-side fake blur**
   - Reject.
   - Forge cannot access pixels behind its Wayland surface.
   - Screen capture/portal hacks would be slow, permission-heavy, visually wrong, and unsafe for a terminal.

2. **Opacity-only**
   - Simple and already mostly exists.
   - Works everywhere alpha compositing is supported.
   - Does not guarantee blur.

3. **KDE `org_kde_kwin_blur` only**
   - Good for KWin/Plasma.
   - Uses existing dependency.
   - Does not help Hyprland/Niri unless they implement that protocol.

4. **Standard/staging `ext-background-effect-v1` only**
   - Architecturally clean.
   - Good future direction.
   - Support is still uneven; Wayland Explorer currently reports no compositor support found for that protocol page, though Niri docs reference app implementation for custom-shaped popups.

5. **Hybrid compositor-native approach**
   - Best.
   - Try `ext-background-effect-v1` when available.
   - Try KDE blur protocol when available.
   - Otherwise rely on transparent surface and document compositor rules for Hyprland/Niri.
   - No renderer-side blur.

## Chosen Approach

Use a **hybrid native blur manager** in the Wayland layer:

1. Keep renderer output translucent via existing opacity/alpha path.
2. Add a `BlurController` owned by `WaylandState` or `WaylandWindow`.
3. Bind supported blur globals during startup:
   - `ext_background_effect_manager_v1` if available.
   - `org_kde_kwin_blur_manager` if available.
4. Apply blur region to the full terminal surface when enabled.
5. On unsupported compositors, do nothing except keep opacity/transparency.
6. Do not redraw continuously because blur is enabled.

Priority order for methods:

```text
auto:
1. ext-background-effect-v1 if available and reports blur capability
2. org_kde_kwin_blur if available
3. transparency-only fallback
```

KDE-specific method can force KDE protocol. External method can mean “do not bind protocols; user configures compositor rules.”

## Architecture Plan

Add Wayland-only blur code under `crates/forge-main/src/wayland/`.

Proposed module:

```text
crates/forge-main/src/wayland/blur.rs
```

Core types:

```rust
enum BlurMethod {
    Auto,
    ExtBackgroundEffect,
    Kde,
    External,
    Disabled,
}

enum ActiveBlurBackend {
    ExtBackgroundEffect { manager, surface },
    Kde { manager, blur },
    External,
    Unsupported,
}

struct BlurController {
    config: BlurConfig,
    backend: ActiveBlurBackend,
    last_region_size: Option<Size>,
    ext_capabilities: u32,
}
```

Responsibilities:

- Bind protocol globals.
- Create/destroy per-surface blur objects.
- Set/unset blur regions.
- Reapply region on resize.
- Handle runtime enable/disable.
- Never touch renderer vertex/shader code except through opacity requirements.

## Config Design

Add:

```lua
blur = {
  enabled = false,
  method = "auto",        -- "auto", "ext-background-effect", "kde", "external", "off"
  radius = nil,           -- advisory only; ignored by most Wayland protocols
  opacity = nil,          -- optional alias/override for window.opacity, or reject to avoid confusion
  region = "window",      -- future: "window", "content", "padding"
}
```

Recommended final config shape:

```lua
window = {
  opacity = 0.85,
}

blur = {
  enabled = true,
  method = "auto",
}
```

Validation:

- `blur.enabled`: bool, default `false`.
- `blur.method`: enum, default `"auto"`.
- `blur.radius`: optional integer, default `nil`; warn that compositor protocols usually ignore client radius.
- Do **not** add `blur.opacity` initially unless there is a clear UX need. Prefer existing `window.opacity`.
- Clamp `window.opacity` as already done: `0.0..=1.0`.
- Blur should warn if enabled with effective opacity `1.0`, because blur may not be visible.

Runtime reload:

- `blur.enabled` / `blur.method`: update Wayland blur state and commit the surface.
- `window.opacity`: already affects render clear; mark all dirty / force redraw.
- Resize: update blur region only when size changes.

Backward compatibility:

- Existing configs remain valid.
- Default blur off means no behavior change.

## File-by-File Implementation Plan

Read again before implementation:

- [connection.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/wayland/connection.rs): globals, dispatch impls, `WaylandState`.
- [window.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/wayland/window.rs): surface ownership and window lifecycle.
- [event_loop.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/event_loop.rs): resize, config reload, opacity, redraw.
- [main.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/main.rs): startup order and first SHM frame.
- [swapchain.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/swapchain.rs): composite alpha.
- [config_registry.rs](/home/kabir/PROJECTS/Forge/crates/forge-core/src/config_registry.rs): config schema/defaults/validation.
- [extractor.rs](/home/kabir/PROJECTS/Forge/crates/forge-config/src/extractor.rs): Lua parsing.
- [types.rs](/home/kabir/PROJECTS/Forge/crates/forge-config/src/types.rs): change-set propagation.

Likely modifications later:

1. `forge-core/src/config_registry.rs`
   - Add `BlurConfig` and `BlurMethod`.
   - Add `blur: BlurConfig` to `ForgeConfig`.
   - Add validation.

2. `forge-config/src/extractor.rs`
   - Parse `blur.enabled`, `blur.method`, optional `blur.radius`.

3. `forge-config/src/types.rs`
   - Add `blur: bool` to `ConfigChangeSet`, or include blur under `window` if kept nested there.

4. `forge-main/src/wayland/connection.rs`
   - Add optional blur globals to `WaylandGlobals`.
   - Add dispatch impls for KDE blur manager/blur.
   - Add dispatch impls for ext-background-effect manager/surface if available in dependency version.
   - Store compositor name only if a reliable protocol/global exposes it; do not guess from env unless only for logs.

5. `forge-main/src/wayland/window.rs`
   - Store `BlurController` in `WaylandWindow` or `WaylandState`.
   - Apply blur after `wl_surface` creation and before/around first commit.

6. `forge-main/src/wayland/blur.rs`
   - New abstraction for protocol-specific blur.
   - Helper to create full-surface `wl_region`.
   - `apply(&surface, size, config)`.
   - `disable(&surface)`.
   - `on_resize(size)`.

7. `forge-main/src/event_loop.rs`
   - On resize: update blur region only if blur enabled and size changed.
   - On config reload: apply/disable blur and force one redraw if opacity changed.
   - Avoid frame wakeups solely for blur after state is committed.

8. `forge-main/Cargo.toml`
   - Already has `wayland-protocols-plasma`.
   - May need `wayland-protocols` version/feature audit for `ext-background-effect`. Current workspace says `0.31`, but local registry has `0.32.12` with `ext::background_effect`. This is an uncertainty to verify during implementation.

9. Renderer files
   - Minimal changes only if alpha is wrong.
   - Audit premultiplied alpha correctness in [event_loop.rs](/home/kabir/PROJECTS/Forge/crates/forge-main/src/event_loop.rs), [pipeline.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/pipeline.rs), and [swapchain.rs](/home/kabir/PROJECTS/Forge/crates/forge-renderer/src/swapchain.rs).

## Compositor Compatibility Plan

- **KDE Plasma/KWin**
  - Preferred: `org_kde_kwin_blur`.
  - Use full-surface region.
  - Radius is compositor-controlled, not app-controlled.
  - Should work best when `window.opacity < 1.0`.

- **Hyprland**
  - Do not rely on app protocol support first.
  - Use transparent surface plus user rules/global blur settings.
  - Document app id: `dev.forge.terminal`.
  - Suggested user-side direction: enable `decoration:blur`, keep Forge translucent, avoid `opaque` / `force_rgbx`, avoid `no_blur`.
  - Hyprland blur settings are compositor-side and include GPU-cost controls like blur size/passes.

- **Niri**
  - Niri supports background-effect rules for windows in config docs since 26.04.
  - If `ext-background-effect` is globally available, use it.
  - Otherwise rely on Niri window rules:
    ```text
    window-rule {
        match app-id="dev.forge.terminal"
        opacity 0.85
        background-effect { blur true }
    }
    ```
  - Note Niri’s non-xray blur limitations during animations.

- **GNOME/Mutter**
  - Expect transparency-only.
  - Do not crash or warn loudly.

- **Sway/river/other wlroots**
  - Expect transparency-only unless compositor-specific support appears.
  - No fake blur.

- **Unknown compositor**
  - `blur.enabled=true` becomes best-effort.
  - Log one debug/info message, not repeated warnings.

## Fallback Behavior

```text
blur disabled:
  no protocol binding required beyond normal startup
  no region creation
  no per-frame work

blur enabled + protocol available:
  apply compositor blur region
  commit once
  keep renderer alpha path

blur enabled + no protocol:
  use transparency only
  log: blur protocol unavailable; compositor rules may be required

opacity == 1.0:
  blur may be invisible
  either warn once or document clearly

protocol object fails:
  unset/destroy local object if possible
  continue running with transparency only
```

## Performance Plan

- No renderer-side blur.
- No extra draw calls.
- No extra frames when idle.
- Create `wl_region` only on enable/resize/config change, never per frame.
- Cache last blur region size.
- Avoid Wayland roundtrips; use globals already discovered by registry.
- Do not query compositor every frame.
- Do not add SHM/Vulkan buffer copies.
- When blur disabled, hot path should be identical to current code except config struct size.

## Testing Plan

Automated:

- Config parsing:
  - default blur disabled
  - parse enabled/method
  - invalid method falls back or warns
  - validation clamps opacity
- Change-set:
  - blur change detected
  - opacity change detected
- Blur controller unit-style tests where possible:
  - no-op when disabled
  - no-op when unsupported
  - resize only reapplies when size changes

Build/tests:

```text
cargo fmt --check
cargo test --workspace
cargo build --workspace
```

Manual verification:

- KDE Plasma:
  - `blur.enabled=true`, `window.opacity=0.85`
  - confirm blur behind Forge
  - resize repeatedly
  - toggle config live if supported
  - disable blur and confirm it disappears
- Hyprland:
  - confirm Forge remains translucent
  - confirm compositor rule can blur by app id
  - ensure `force_rgbx`/`opaque` rules are not required and should be avoided
- Niri:
  - test window-rule background-effect
  - test app protocol if available
- GNOME/Sway/river:
  - confirm no crash
  - transparency-only fallback
- Performance:
  - idle CPU/GPU before/after with blur disabled
  - idle CPU/GPU with blur enabled
  - fast output
  - resize
  - `nvim`, `btop`, `tmux`
- Visual:
  - background visible through terminal
  - text remains crisp
  - padding-fill still correct
  - scrollbar still correct
  - decorations/shadows do not produce obvious rectangular artifacts

## Risks and Unknowns

- `ext-background-effect-v1` availability in the current `wayland-protocols` dependency version is uncertain. Local registry has it in `0.32.12`; workspace currently declares `0.31`.
- KWin blur protocol does not expose radius. Any `blur.radius` config would be advisory or ignored.
- Hyprland blur is mostly user compositor configuration, not an app-controlled portable API.
- Niri support depends on version and rule/protocol availability.
- Alpha correctness needs careful visual testing. Existing code uses premultiplied clear color, but foreground/background quads and swapchain alpha need an audit before assuming perfect compositor blending.
- Server-side decorations may affect perceived blur boundaries.
- Fully opaque cell backgrounds inside the terminal can visually hide blur. For a blurred terminal look, default background clear should be translucent, while text/cell backgrounds must remain readable.

## Final Step-by-Step Implementation Order

1. Add `BlurConfig` and `BlurMethod` to core config with defaults disabled.
2. Parse `blur` in Lua config.
3. Add config validation and tests.
4. Add `wayland/blur.rs` abstraction with no-op backend first.
5. Bind KDE blur global in `WaylandGlobals`.
6. Implement KDE blur apply/unset with full-surface region.
7. Add runtime resize/config hooks to apply/update/unset blur.
8. Audit Vulkan/SHM alpha behavior and fix only if proven wrong.
9. Add `ext-background-effect-v1` support if dependency version supports it cleanly.
10. Add fallback logging and compositor guidance.
11. Run full tests.
12. Manually verify on KDE.
13. Manually verify Hyprland via compositor rules.
14. Manually verify Niri via window rules / protocol if available.
15. Verify fallback compositors do not regress.
