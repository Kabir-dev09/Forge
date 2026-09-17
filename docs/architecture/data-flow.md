# Forge Data Flow Analysis

This document traces the path of input, output, and terminal state through the Forge architecture.

## 1. Input Flow (Keyboard to PTY)

```mermaid
sequenceDiagram
    participant Wayland Compositor
    participant UI Thread (seat.rs)
    participant Channel
    participant PTY Worker (io.rs)
    participant OS PTY Master

    Wayland Compositor->>UI Thread (seat.rs): wl_keyboard::Event::Key
    UI Thread (seat.rs)->>UI Thread (seat.rs): xkbcommon translates to keysym/utf8
    UI Thread (seat.rs)->>UI Thread (seat.rs): Check against Keybindings
    alt Match found
        UI Thread (seat.rs)->>UI Thread (seat.rs): Queue Action (e.g. SplitVertical)
    else No Match
        UI Thread (seat.rs)->>UI Thread (seat.rs): Encode to escape sequence or raw bytes
        UI Thread (seat.rs)->>Channel: tx.send(bytes)
    end
    UI Thread (seat.rs)->>PTY Worker (io.rs): enqueue PtyWorkerCommand::Write(pane, bytes)
    PTY Worker (io.rs)->>PTY Worker (io.rs): calloop awakes
    PTY Worker (io.rs)->>OS PTY Master: write(fd, bytes)
```

## 2. Output Flow (PTY to ScreenBuffer)

```mermaid
sequenceDiagram
    participant OS PTY Master
    participant PTY Worker (io.rs)
    participant VteProcessor
    participant ScreenBuffer
    participant Snapshot

    OS PTY Master->>PTY Worker (io.rs): fd readable (calloop event)
    PTY Worker (io.rs)->>PTY Worker (io.rs): read(fd) into buffer
    PTY Worker (io.rs)->>VteProcessor: process(buffer, &mut ScreenBuffer)
    alt Fast Path
        VteProcessor->>ScreenBuffer: batch insert ASCII & newlines
    else Escape Sequence
        VteProcessor->>VteProcessor: vte state machine parses
        VteProcessor->>ScreenBuffer: TerminalPerformer mutates (e.g., SGR color)
    end
    PTY Worker (io.rs)->>ScreenBuffer: increment dirty_generations for modified rows
    PTY Worker (io.rs)->>Snapshot: ScreenBuffer::generate_snapshot()
    PTY Worker (io.rs)->>Snapshot: arc_swap::store() (Lock-free publish)
```

## 3. Rendering Flow (State to Screen)

```mermaid
sequenceDiagram
    participant UI Thread (event_loop.rs)
    participant Snapshot
    participant GridTessellator
    participant Renderer (renderer.rs)
    participant GPU

    UI Thread (event_loop.rs)->>UI Thread (event_loop.rs): Event Loop Tick (Frame Ready)
    UI Thread (event_loop.rs)->>Snapshot: arc_swap::load() (Lock-free read)
    UI Thread (event_loop.rs)->>GridTessellator: Compare snapshot dirty_generations
    GridTessellator->>GridTessellator: Build vertices for changed rows
    GridTessellator->>GridTessellator: Lookup glyph UVs in GlyphAtlas
    alt Missing Glyph
        GridTessellator->>GridTessellator: Add to missing_glyphs set
    end
    UI Thread (event_loop.rs)->>Renderer (renderer.rs): submit_tessellated_vertices()
    alt Has Missing Glyphs
        Renderer (renderer.rs)->>Renderer (renderer.rs): insert_dynamic_glyphs()
        Renderer (renderer.rs)->>GPU: atlas_texture.update_regions()
    end
    Renderer (renderer.rs)->>GPU: Copy vertices to mapped memory
    Renderer (renderer.rs)->>GPU: Record Command Buffer (Draw)
    Renderer (renderer.rs)->>GPU: Queue Submit
```
