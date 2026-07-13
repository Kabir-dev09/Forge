import re
with open('/home/kabir/PROJECTS/Forge/crates/forge-main/src/event_loop.rs', 'r') as f:
    content = f.read()

# 1. Fix BUG-01
old_bug01 = """                                        let new_offset =
                                            (scroll_ratio * history_lines).round() as usize;

                                        // TODO: handle scrollbar drag
                                        app_data.loop_signal.wakeup();"""
new_bug01 = """                                        let new_offset =
                                            (scroll_ratio * history_lines).round() as usize;

                                        app_data.pane_io.send_ui_command(
                                            crate::mux::io::PtyWorkerCommand::SetScrollOffset(
                                                active_pane,
                                                new_offset,
                                            ),
                                        );
                                        app_data.loop_signal.wakeup();"""
if "// TODO: handle scrollbar drag" in content:
    content = content.replace(old_bug01, new_bug01)


# 2. Fix BUG-07
old_bug07 = """                                PaneAnimationKind::Open => {
                                    if app_data.config.render.pane_animation == forge_core::config_registry::PaneAnimationMode::Fade {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x,
                                            y: span.rect.y,
                                            width: span.rect.width,
                                            height: span.rect.height,
                                        }, p)
                                    } else {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x + anim.dx * inv,
                                            y: span.rect.y + anim.dy * inv,
                                            width: (span.rect.width + anim.dw * inv).max(0.0),
                                            height: (span.rect.height + anim.dh * inv).max(0.0),
                                        }, 1.0)
                                    }
                                }
                                _ => {"""

new_bug07 = """                                PaneAnimationKind::Open => {
                                    if app_data.config.render.pane_animation == forge_core::config_registry::PaneAnimationMode::Fade {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x,
                                            y: span.rect.y,
                                            width: span.rect.width,
                                            height: span.rect.height,
                                        }, p)
                                    } else {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x + anim.dx * inv,
                                            y: span.rect.y + anim.dy * inv,
                                            width: (span.rect.width + anim.dw * inv).max(0.0),
                                            height: (span.rect.height + anim.dh * inv).max(0.0),
                                        }, 1.0)
                                    }
                                }
                                PaneAnimationKind::Close => {
                                    if app_data.config.render.pane_animation == forge_core::config_registry::PaneAnimationMode::Fade {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x,
                                            y: span.rect.y,
                                            width: span.rect.width,
                                            height: span.rect.height,
                                        }, inv)
                                    } else {
                                        (forge_renderer::renderer::PaneRenderRect {
                                            x: span.rect.x + anim.dx * p,
                                            y: span.rect.y + anim.dy * p,
                                            width: (span.rect.width + anim.dw * p).max(0.0),
                                            height: (span.rect.height + anim.dh * p).max(0.0),
                                        }, 1.0)
                                    }
                                }
                                _ => {"""
content = content.replace(old_bug07, new_bug07)

# 3. Fix RISK-07 (Mouse SGR modes 1000/1002)
# There's a mouse mode parsing somewhere.
# Wait, let's look for "mouse_sgr_mode".
# I'll do this one manually or add it later.

# 4. Fix PERF-06 (set_ligature_config cloning every frame)
# Replace `renderer.set_ligature_config(app_data.config.font.ligatures.clone());` with nothing, because it's handled on config change.
old_perf06 = "renderer.set_ligature_config(app_data.config.font.ligatures.clone());"
content = content.replace(old_perf06, "")

# 5. Fix RISK-01 (unwrap on active pane)
# Replace `.get(&active_pane).unwrap()` with `.get(&active_pane)?` inside Option contexts or handled otherwise?
# It's better to replace them carefully.
unwrap_pattern = re.compile(r'\.get\(&([a-zA-Z0-9_]+)\)\s*\n*\s*\.unwrap\(\)')
# For each match, we can use an if let block or if it's returning, we might need manual fix.
# Let's save the file.

with open('/home/kabir/PROJECTS/Forge/crates/forge-main/src/event_loop.rs', 'w') as f:
    f.write(content)
