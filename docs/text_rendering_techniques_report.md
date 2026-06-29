# Text Rendering Techniques Research Report

Date: 2026-06-25

Project context: Forge is a terminal emulator with a Vulkan renderer. Its current text path is a pre-rasterized glyph atlas: glyphs are rasterized at a target pixel size, packed into a texture atlas, and rendered as quads. Recent rendering work has also separated cell layout scaling from glyph bitmap scaling so glyphs can remain pixel-snapped and crisp.

## Executive Summary

For a terminal emulator, the best general-purpose text rendering strategy is not the most advanced one. It is a carefully implemented rasterized glyph atlas pipeline backed by a high-quality shaping and rasterization stack.

The best practical approach for Forge is:

1. Keep the pre-rasterized/dynamic glyph atlas model.
2. Improve rasterization quality, shaping, fallback, cache invalidation, and DPI handling.
3. Keep glyph quads pixel-snapped.
4. Never scale already-rasterized glyph bitmaps for layout fill.
5. Render box-drawing, block elements, braille, and powerline symbols procedurally or with terminal-specific glyph handling.
6. Add persistent glyph cache support only after the runtime pipeline is correct.

Replacing the current atlas path with SDF, MSDF, or GPU vector rendering is not recommended for terminal body text. Those techniques are valuable in games, UI scaling systems, map labels, editors with zoomable canvases, and vector-heavy graphics engines, but they are usually worse than native-size rasterization for small, dense, monospace terminal text.

## Evaluation Criteria

Ratings are from 1 to 10 for Forge's needs, not for general graphics use. The scoring emphasizes:

- Crisp small text.
- Low CPU and GPU cost.
- Low memory cost.
- Fast startup.
- Smooth resize and DPI behavior.
- Unicode and fallback support.
- Box-drawing correctness.
- Maintainability.
- Production practicality.

## Current Forge Baseline: Pre-Rasterized Glyph Atlas

How it works:

- A font rasterizer produces bitmap coverage masks for glyphs at a specific font size.
- Glyph bitmaps are packed into a texture atlas.
- The renderer draws one quad per visible glyph with atlas UVs.
- Backgrounds, selections, cursor, and procedural symbols are drawn separately.

Where it wins:

- Excellent crispness when rasterized at the exact physical pixel size.
- Very low GPU cost: simple textured quads.
- Good CPU performance after the atlas is built.
- Simple shader.
- Predictable memory usage for ASCII and common glyph ranges.
- Fits terminal rendering well because terminals use stable cell metrics and repeated glyphs.

Where it loses:

- Needs atlas rebuilds or multiple atlases for font size and DPI changes.
- Large Unicode coverage can increase startup time and memory use.
- Complex scripts require shaping support; raw codepoint-to-glyph mapping is not enough.
- Fallback fonts complicate atlas keys and glyph ownership.
- Bitmap glyphs cannot be scaled without blur or aliasing.

Current quality rating for the approach: 8.5/10.

Potential Forge rating after improvements: 9/10.

## Technique Rankings For Forge

| Rank | Technique | Forge Suitability | Rating |
|---:|---|---:|---:|
| 1 | Dynamic rasterized glyph atlas with shaping and fallback | Excellent | 9.0 |
| 2 | Pre-rasterized glyph atlas with persistent cache | Excellent | 8.8 |
| 3 | Runtime rasterization plus atlas cache | Very good | 8.5 |
| 4 | Procedural terminal symbols plus rasterized text | Very good as hybrid | 8.5 |
| 5 | OS/native text stack integration | Good, platform-dependent | 8.0 |
| 6 | Static bitmap font rendering | Good for low-resource or pixel fonts | 7.0 |
| 7 | Subpixel/LCD rasterized atlas | Good only under strict display assumptions | 6.8 |
| 8 | CPU-side text compositing | Useful fallback, poor main path | 6.0 |
| 9 | Hybrid raster/vector pipeline | Powerful but complex | 6.0 |
| 10 | SDF fonts | Poor for small terminal body text | 5.5 |
| 11 | MSDF/MTSDF fonts | Better scalable graphics, still weak for small text | 5.5 |
| 12 | GPU vector/path text rendering | High quality potential, too complex | 5.0 |
| 13 | Pure vector tessellated glyph meshes | Niche, usually overkill | 4.5 |
| 14 | Per-frame CPU rasterization without atlas | Bad for terminal workloads | 3.5 |

## Detailed Technique Analysis

### 1. Basic Bitmap Font Rendering

How it works:

- Glyphs are stored as fixed-size bitmaps, often one bitmap per character at one pixel size.
- Rendering copies pixels or samples a bitmap atlas.
- Fonts may be BDF, PCF, FON, embedded ROM fonts, or custom bitmap sheets.

Common usage:

- Embedded systems.
- BIOS/firmware consoles.
- Retro games.
- Low-resource UI.
- Pixel-art tools.

Text sharpness:

- Extremely sharp at the designed pixel size.
- Poor when scaled.
- No smooth curves unless the bitmap was designed that way.

CPU cost:

- Very low.

GPU cost:

- Very low if using an atlas.

Memory usage:

- Low for ASCII.
- Medium to high for Unicode if many sizes are included.

Startup cost:

- Very low.

Runtime cost:

- Very low.

Scaling and DPI:

- Weak. Requires separate bitmap strikes for each size and DPI.

Unicode support:

- Usually limited unless a large bitmap font set is provided.

Box-drawing and terminal compatibility:

- Excellent if the bitmap font includes terminal symbols at exact cell dimensions.

Implementation complexity:

- Low.

Maintainability:

- Good for fixed environments.
- Poor for arbitrary user fonts.

Advantages:

- Sharpest possible pixel-perfect output at one size.
- Simple and fast.
- Predictable metrics.

Disadvantages:

- Inflexible.
- Weak modern font support.
- Poor for user-selected fonts, ligatures, emoji, fallback, and HiDPI scaling.

Comparison with pre-rasterized atlas:

- Bitmap fonts are simpler and can be sharper at a single size.
- The atlas approach supports real user fonts, more sizes, more Unicode, and better antialiasing.

Terminal suitability:

- Good as an optional "pixel font" mode.
- Not suitable as the default modern renderer.

Rating: 7/10.

### 2. Runtime Rasterization

How it works:

- The application loads a font file and rasterizes glyphs on demand or during startup.
- Rasterization may use FreeType, CoreText, DirectWrite, fontdue, swash, stb_truetype, or platform APIs.
- The resulting bitmap is drawn directly or inserted into an atlas.

Common usage:

- Browsers.
- UI frameworks.
- Editors.
- Games.
- Terminal emulators.

Text sharpness:

- High when using correct hinting, grayscale antialiasing, pixel snapping, and target-size rasterization.
- Can be poor if glyphs are scaled or placed on fractional pixels.

CPU cost:

- Medium during glyph rasterization.
- Low after caching.

GPU cost:

- Low when rendering cached quads.

Memory usage:

- Depends on cache size and glyph coverage.

Startup cost:

- Low to medium. Higher if preloading large Unicode ranges.

Runtime cost:

- Low after glyphs are cached.
- Short spikes if many missing glyphs are rasterized at once.

Scaling and DPI:

- Good if atlases are rebuilt per physical pixel size and DPI.

Unicode support:

- Good with shaping and fallback.
- Weak if only codepoint-to-glyph lookup is implemented.

Box-drawing and terminal compatibility:

- Good if box drawing is either present in the font or rendered procedurally.

Implementation complexity:

- Medium.

Maintainability:

- Good if using proven libraries.

Advantages:

- Flexible.
- Supports user fonts.
- Good quality.
- Natural fit for dynamic terminal content.

Disadvantages:

- Requires cache management.
- Requires shaping/fallback work for full Unicode.
- Startup can suffer if too many glyphs are preloaded.

Comparison with pre-rasterized atlas:

- Runtime rasterization is the producer side of the current atlas approach.
- It is not a replacement; it is how the atlas should be populated.

Terminal suitability:

- Very high.

Rating: 8.5/10.

### 3. Pre-Rasterized Glyph Atlas

How it works:

- A set of glyphs is rasterized ahead of rendering and packed into one or more texture atlases.
- The renderer draws glyph quads using atlas coordinates.

Common usage:

- Games.
- Terminal emulators.
- Immediate-mode UI.
- Text overlays.
- Embedded UIs.

Text sharpness:

- Excellent at the exact rasterized pixel size.
- Poor if scaled.

CPU cost:

- Medium at atlas build time.
- Very low during rendering.

GPU cost:

- Very low.

Memory usage:

- Low to medium for ASCII and common symbols.
- High for large preloaded Unicode ranges.

Startup cost:

- Can be medium or high if too many glyphs are preloaded.

Runtime cost:

- Low.

Scaling and DPI:

- Requires separate atlases per size/DPI.

Unicode support:

- Good if combined with dynamic fallback.
- Poor if only one static range is included.

Box-drawing and terminal compatibility:

- Good, but procedural symbols are better for perfect joins.

Implementation complexity:

- Medium.

Maintainability:

- Good.

Advantages:

- Simple render path.
- Excellent performance.
- Predictable output.
- Good fit for dense terminal grids.

Disadvantages:

- Cache invalidation complexity.
- Size/DPI-specific.
- Large Unicode preloads are expensive.

Comparison with Forge:

- This is Forge's current core approach.
- It should be improved, not replaced.

Terminal suitability:

- Excellent.

Rating: 8.8/10.

### 4. Dynamic Glyph Atlas

How it works:

- Glyphs are rasterized and inserted into an atlas only when first needed.
- The atlas can grow, evict old glyphs, or allocate additional pages.
- Atlas keys include font face, size, style, DPI, glyph ID, color format, and rasterization mode.

Common usage:

- Browsers.
- Game engines.
- UI frameworks.
- Editors.
- Modern terminal renderers.

Text sharpness:

- Excellent when each glyph is rasterized at the exact target pixel size.

CPU cost:

- Low average.
- Occasional spikes on glyph misses.

GPU cost:

- Low.

Memory usage:

- Better than preloading all Unicode.
- Needs eviction or paging policy.

Startup cost:

- Low if only ASCII/common glyphs are preloaded.

Runtime cost:

- Low after warmup.

Scaling and DPI:

- Good with per-size/per-DPI atlas pages.

Unicode support:

- Very good with fallback and shaping.

Box-drawing and terminal compatibility:

- Very good if terminal symbols are handled procedurally or prewarmed.

Implementation complexity:

- Medium to high.

Maintainability:

- Good if the atlas API is clean.

Advantages:

- Fast startup.
- Scales to large Unicode without huge upfront cost.
- Good performance.

Disadvantages:

- More complex than a fixed atlas.
- Needs upload synchronization.
- Needs miss handling.

Comparison with pre-rasterized atlas:

- Dynamic atlas is a more scalable version of the current approach.
- It wins on startup and Unicode coverage.
- It loses on implementation simplicity.

Terminal suitability:

- Best overall for Forge.

Rating: 9/10.

### 5. Texture Atlas Rendering

How it works:

- Glyphs, icons, or symbols are stored in one or more textures.
- Quads reference UV coordinates into the atlas.
- The atlas may be static, dynamic, or persistent.

Common usage:

- Almost every GPU text renderer.
- Games.
- Browsers.
- UI frameworks.
- Terminal emulators.

Text sharpness:

- Depends on source glyph quality and sampling.
- Excellent if no scaling and correct pixel alignment.

CPU cost:

- Low.

GPU cost:

- Low.

Memory usage:

- Medium.

Startup cost:

- Depends on atlas generation.

Runtime cost:

- Low.

Scaling and DPI:

- Requires size-specific atlas pages.

Unicode support:

- Depends on atlas management.

Box-drawing and terminal compatibility:

- Good; procedural terminal symbols can share the same render pipeline.

Implementation complexity:

- Medium.

Maintainability:

- Good.

Advantages:

- Efficient.
- Simple shaders.
- Works well with batching.

Disadvantages:

- Atlas fragmentation.
- Eviction complexity.
- Texture upload synchronization.

Comparison with pre-rasterized atlas:

- Texture atlas rendering is the GPU side of the current approach.

Terminal suitability:

- Excellent.

Rating: 8.8/10.

### 6. FreeType/CoreText/DirectWrite-Style Rasterization

How it works:

- A mature font engine handles glyph loading, metrics, hinting, antialiasing, variations, color glyphs, and sometimes shaping/layout.
- FreeType exposes glyph loading and rendering APIs. Its documentation highlights that hinting changes glyph dimensions and metrics, and that hinted glyphs should be translated by integer pixel distances for best quality.
- DirectWrite provides high-quality text rendering, subpixel ClearType, hardware acceleration with Direct2D, OpenType support, and Unicode layout support.

Common usage:

- Operating systems.
- Browsers.
- Desktop UI frameworks.
- Editors.
- Professional applications.

Text sharpness:

- High to excellent.
- Depends on configuration, hinting mode, antialiasing, and display.

CPU cost:

- Medium for rasterization and shaping.
- Low after caching.

GPU cost:

- Low when cached into an atlas.

Memory usage:

- Medium.

Startup cost:

- Low to medium.

Runtime cost:

- Low with caching.

Scaling and DPI:

- Excellent if using platform APIs correctly.

Unicode support:

- Excellent with shaping and fallback.

Box-drawing and terminal compatibility:

- Good, but terminal-specific procedural symbols may still be required.

Implementation complexity:

- Medium to high, especially cross-platform.

Maintainability:

- Good if the dependency boundary is clean.

Advantages:

- Mature.
- High quality.
- Excellent script and font support.
- Uses decades of font engineering.

Disadvantages:

- Platform differences.
- More dependency complexity.
- DirectWrite/CoreText are not portable in the same way as FreeType/HarfBuzz.

Comparison with pre-rasterized atlas:

- These engines should feed the atlas rather than replace it.
- They improve glyph quality, metrics, shaping, fallback, and DPI behavior.

Terminal suitability:

- Excellent as the rasterization/shaping backend.

Rating: 8.8/10.

### 7. Grayscale Antialiasing

How it works:

- Glyph coverage is represented as one alpha value per pixel.
- The glyph is blended over the background.

Common usage:

- Cross-platform UI.
- Browsers.
- macOS after subpixel rendering removal.
- Many game/UI text systems.

Text sharpness:

- High on HiDPI.
- Good on normal DPI if hinting and snapping are correct.
- Softer than LCD subpixel rendering on low-DPI panels.

CPU cost:

- Low to medium during rasterization.

GPU cost:

- Low.

Memory usage:

- Low: one channel can be enough.

Startup cost:

- Low to medium.

Runtime cost:

- Low.

Scaling and DPI:

- Good if rasterized per target size.

Unicode support:

- Depends on shaping and fallback.

Box-drawing and terminal compatibility:

- Good, but font-rendered box drawing can misalign; procedural is better.

Implementation complexity:

- Low to medium.

Maintainability:

- Good.

Advantages:

- Portable.
- No RGB subpixel assumptions.
- Good for rotated displays and mixed monitors.

Disadvantages:

- Slightly softer than LCD rendering at low DPI.

Comparison with pre-rasterized atlas:

- Grayscale AA is usually the coverage format inside the atlas.

Terminal suitability:

- Very good default.

Rating: 8.5/10.

### 8. Hinting-Based Rendering

How it works:

- Font instructions or auto-hinting adjust outlines to align stems and important features to the pixel grid.
- Hinting is resolution-dependent and changes glyph metrics.

Common usage:

- FreeType.
- Windows GDI/DirectWrite.
- Low-DPI UI text.

Text sharpness:

- Excellent at small sizes when configured well.
- Can distort typeface shape.

CPU cost:

- Medium during glyph loading/rasterization.

GPU cost:

- None beyond normal bitmap rendering.

Memory usage:

- No major extra memory after rasterization.

Startup cost:

- Slightly higher than unhinted rasterization.

Runtime cost:

- Low after caching.

Scaling and DPI:

- Good at target sizes.
- Requires rerasterization per size/DPI.

Unicode support:

- Depends on font hints and fallback.

Box-drawing and terminal compatibility:

- Useful for text.
- Box drawing should often bypass font hinting and be procedural.

Implementation complexity:

- Medium if using a library; high if implemented manually.

Maintainability:

- Good with FreeType/DirectWrite/CoreText.

Advantages:

- Improves small text clarity.
- Standard professional technique.

Disadvantages:

- Can change glyph advances.
- Requires careful metric handling.
- Bad if glyphs are later fractionally translated.

Comparison with pre-rasterized atlas:

- Hinting should be applied before atlas insertion.
- The atlas must preserve the hinted result without scaling.

Terminal suitability:

- Very good.

Rating: 8.5/10.

### 9. Subpixel/LCD Text Rendering

How it works:

- Uses RGB/BGR subpixel layout to increase apparent horizontal resolution.
- Produces separate coverage values for color subpixels.
- Often paired with filtering to reduce color fringes.

Common usage:

- Windows ClearType.
- Older desktop Linux configurations.
- Some FreeType configurations.

Text sharpness:

- Excellent on standard RGB low-DPI panels.
- Can be worse on rotated displays, unusual subpixel layouts, transparent surfaces, or composited rendering.

CPU cost:

- Medium during rasterization.

GPU cost:

- Slightly higher due to RGB masks and special blending.

Memory usage:

- Higher than grayscale if storing RGB coverage.

Startup cost:

- Medium.

Runtime cost:

- Low after caching.

Scaling and DPI:

- Weak across mixed monitor subpixel layouts.
- Less useful on HiDPI.

Unicode support:

- Depends on shaping and fallback.

Box-drawing and terminal compatibility:

- Good for text.
- Risky for colored terminal backgrounds because LCD masks assume known background and subpixel geometry.

Implementation complexity:

- High for correct blending.

Maintainability:

- Medium to low cross-platform.

Advantages:

- Very sharp on compatible low-DPI screens.

Disadvantages:

- Color fringing.
- Display-layout dependency.
- Hard with transparency and arbitrary background colors.
- Less attractive on modern HiDPI.

Comparison with pre-rasterized atlas:

- It can be an atlas format variant.
- It should not replace the atlas model.

Terminal suitability:

- Optional advanced mode only.

Rating: 6.8/10.

### 10. CPU-Side Text Compositing

How it works:

- Glyphs are rasterized and blended into a CPU framebuffer.
- The final image is uploaded to the GPU or displayed directly.

Common usage:

- Software renderers.
- Embedded systems.
- Remote rendering.
- Some old UI frameworks.

Text sharpness:

- High if using a good rasterizer.

CPU cost:

- High for full-screen updates.

GPU cost:

- Low.

Memory usage:

- Full framebuffer plus glyph cache.

Startup cost:

- Low to medium.

Runtime cost:

- High for terminals with frequent scrolling.

Scaling and DPI:

- Good if rerasterized.

Unicode support:

- Good with shaping/fallback.

Box-drawing and terminal compatibility:

- Good.

Implementation complexity:

- Medium.

Maintainability:

- Good but performance-limited.

Advantages:

- Simple correctness model.
- Easy screenshots and tests.

Disadvantages:

- Poor for high-refresh terminal scrolling.
- Wastes CPU bandwidth.

Comparison with pre-rasterized atlas:

- Loses strongly on GPU-accelerated terminals.
- Useful as fallback or test renderer.

Terminal suitability:

- Not recommended as main path.

Rating: 6/10.

### 11. GPU Instanced Glyph Rendering

How it works:

- Each glyph is represented by a small instance record: position, glyph ID/UV, colors, style flags.
- A vertex shader expands instances into quads or indexed vertices.
- Reduces CPU-side vertex generation and memory upload.

Common usage:

- Game engines.
- UI engines.
- High-performance text renderers.

Text sharpness:

- Same as atlas source.

CPU cost:

- Lower than pushing six vertices per glyph.

GPU cost:

- Low.

Memory usage:

- Lower per glyph than full quads.

Startup cost:

- No major difference.

Runtime cost:

- Lower upload bandwidth.

Scaling and DPI:

- Same as atlas pipeline.

Unicode support:

- Same as atlas pipeline.

Box-drawing and terminal compatibility:

- Good; procedural glyphs can use instance flags.

Implementation complexity:

- Medium.

Maintainability:

- Good if the pipeline is clean.

Advantages:

- Efficient.
- Reduces CPU tessellation and upload size.
- Good next step after correctness.

Disadvantages:

- Requires shader and pipeline changes.
- More complex batching.

Comparison with pre-rasterized atlas:

- It improves how the atlas is drawn.
- It does not replace the atlas.

Terminal suitability:

- Very good performance optimization.

Rating: 8.5/10 as an optimization, not a standalone technique.

### 12. Signed Distance Field Fonts

How it works:

- A texture stores distance to the nearest glyph edge rather than direct coverage.
- The shader reconstructs coverage using a threshold and smoothing range.

Common usage:

- Games.
- 3D labels.
- Scalable UI.
- Map labels.

Text sharpness:

- Good at medium/large sizes.
- Often poor for small dense text.
- Thin strokes and punctuation can degrade.

CPU cost:

- Medium to generate.

GPU cost:

- Slightly higher shader cost.

Memory usage:

- Can be lower than multiple bitmap sizes.

Startup cost:

- Medium or high if generated at startup.

Runtime cost:

- Low to medium.

Scaling and DPI:

- Good for scaling.

Unicode support:

- Possible but large atlases or dynamic generation are still needed.

Box-drawing and terminal compatibility:

- Weak for pixel-perfect box drawing unless symbols are procedural.

Implementation complexity:

- Medium.

Maintainability:

- Medium.

Advantages:

- Scales better than bitmaps.
- Useful for zoomable or transformed text.

Disadvantages:

- Worse than native rasterization for small terminal text.
- Loses hinting.
- Can look synthetic.

Comparison with pre-rasterized atlas:

- SDF wins when text is scaled continuously.
- The atlas wins for fixed-size crisp terminal text.

Terminal suitability:

- Not recommended for main text.

Rating: 5.5/10.

### 13. Multi-Channel Signed Distance Fields

How it works:

- Stores multiple distance fields in RGB channels to better preserve sharp corners and edge intersections.
- MSDF tools generate a distance representation from vector outlines.

Common usage:

- Game engines.
- UI labels.
- Vector icons.
- Large scalable text.

Text sharpness:

- Better than SDF for corners and large sizes.
- Still not ideal for small hinted terminal text.

CPU cost:

- High to generate.

GPU cost:

- Medium.

Memory usage:

- Higher than single-channel SDF.

Startup cost:

- High if generated at startup.

Runtime cost:

- Low to medium.

Scaling and DPI:

- Good.

Unicode support:

- Challenging at large coverage.

Box-drawing and terminal compatibility:

- Better handled procedurally.

Implementation complexity:

- High.

Maintainability:

- Medium to low for a terminal.

Advantages:

- Good scalable vector-like text.
- Useful for large UI and game text.

Disadvantages:

- Small text quality is not as good as proper rasterization.
- More artifacts and shader complexity.
- Not a strong fit for terminal grids.

Comparison with pre-rasterized atlas:

- MSDF wins for scalable display text.
- The atlas wins for body text and terminal workloads.

Terminal suitability:

- Poor as default.

Rating: 5.5/10.

### 14. Vector/Path-Based GPU Rendering

How it works:

- Glyph outlines are rendered directly as vector paths.
- The GPU evaluates coverage analytically, through tessellation, stencil-and-cover, compute rasterization, or specialized path-rendering algorithms.

Common usage:

- Vector graphics systems.
- PDF viewers.
- High-end graphics engines.
- Experimental renderers.

Text sharpness:

- Potentially excellent.
- Small text still needs hinting/grid fitting to match rasterized quality.

CPU cost:

- Medium to high depending on tessellation and caching.

GPU cost:

- Medium to high.

Memory usage:

- Can be lower than many bitmap sizes.
- Needs outline/path caches.

Startup cost:

- Medium.

Runtime cost:

- Medium to high for many glyphs.

Scaling and DPI:

- Excellent in theory.

Unicode support:

- Possible with shaping/fallback, but glyph path caching becomes complex.

Box-drawing and terminal compatibility:

- Procedural symbols still better.

Implementation complexity:

- Very high.

Maintainability:

- Low unless using a mature library.

Advantages:

- Resolution independent.
- Good for arbitrary transforms.

Disadvantages:

- Overkill for terminal text.
- Hard to make as crisp as hinted rasterization at small sizes.
- Complex and risky.

Comparison with pre-rasterized atlas:

- Vector wins for zoom and arbitrary transforms.
- Atlas wins decisively for fixed-size terminal rendering.

Terminal suitability:

- Not recommended.

Rating: 5/10.

### 15. Pure Glyph Mesh Tessellation

How it works:

- Font outlines are tessellated into triangle meshes.
- Meshes are drawn with normal GPU rasterization.

Common usage:

- 3D engines.
- Extruded text.
- CAD/visualization.

Text sharpness:

- Good at large sizes.
- Weak for small hinted text unless special AA is added.

CPU cost:

- High during tessellation.

GPU cost:

- Medium.

Memory usage:

- High for many glyph meshes.

Startup cost:

- Medium to high.

Runtime cost:

- Medium.

Scaling and DPI:

- Good for geometric scale.

Unicode support:

- Possible but memory-heavy.

Box-drawing and terminal compatibility:

- Not a good match.

Implementation complexity:

- High.

Maintainability:

- Medium to low.

Advantages:

- Works in 3D.
- No texture atlas required.

Disadvantages:

- Poor fit for dense 2D text.
- Needs antialiasing strategy.

Comparison with pre-rasterized atlas:

- Atlas is much better for terminals.

Terminal suitability:

- Poor.

Rating: 4.5/10.

### 16. Hybrid Raster/Vector Pipeline

How it works:

- Small text uses rasterized glyph atlases.
- Large or transformed text uses vector paths, SDF, or MSDF.
- Terminal symbols may be procedural.

Common usage:

- Browsers.
- Graphics engines.
- UI systems with zoom.
- Editors with minimaps or zoomable surfaces.

Text sharpness:

- High if the switch points are well chosen.

CPU cost:

- Medium.

GPU cost:

- Medium.

Memory usage:

- Medium to high.

Startup cost:

- Medium.

Runtime cost:

- Medium.

Scaling and DPI:

- Excellent.

Unicode support:

- Good with shaping/fallback.

Box-drawing and terminal compatibility:

- Excellent if terminal symbols are procedural.

Implementation complexity:

- High.

Maintainability:

- Medium.

Advantages:

- Best of multiple worlds.
- Good for applications with many text scales.

Disadvantages:

- More code paths.
- More tests.
- Not necessary for normal terminal use.

Comparison with pre-rasterized atlas:

- Hybrid extends the atlas approach.
- It is not needed until Forge has zoom, tabs with UI text, or complex overlays.

Terminal suitability:

- Good long-term architecture, excessive near-term.

Rating: 6/10.

### 17. Procedural Terminal Symbol Rendering

How it works:

- Terminal-specific symbols are rendered using shader logic or generated geometry instead of font bitmaps.
- Examples: box-drawing lines, block elements, braille, powerline separators, cursor shapes.

Common usage:

- Modern terminals.
- Code editors.
- Terminal UI renderers.

Text sharpness:

- Excellent for line art and cell-filling symbols.

CPU cost:

- Low.

GPU cost:

- Low to medium depending on shader complexity.

Memory usage:

- Very low.

Startup cost:

- None.

Runtime cost:

- Low.

Scaling and DPI:

- Excellent if based on cell geometry and physical pixels.

Unicode support:

- Limited to known symbol ranges.

Box-drawing and terminal compatibility:

- Excellent.

Implementation complexity:

- Medium.

Maintainability:

- Good if range handling is clean.

Advantages:

- Perfect joins.
- Avoids font inconsistencies.
- Avoids missing glyphs.
- Great for terminal borders.

Disadvantages:

- Not suitable for general text.
- Requires per-symbol logic.

Comparison with pre-rasterized atlas:

- It complements the atlas.
- It should replace atlas rendering for terminal graphics ranges, not for normal text.

Terminal suitability:

- Excellent hybrid component.

Rating: 8.5/10 as a terminal-specific supplement.

### 18. Persistent Glyph Cache / Custom Preprocessed Font Cache

How it works:

- Glyph metrics, rasterized bitmaps, atlas pages, or shaping metadata are cached to disk.
- Cache keys include font file hash, size, DPI, renderer version, rasterization mode, and fallback face.

Common usage:

- Browsers.
- Game engines.
- Large applications.
- Some custom UI systems.

Text sharpness:

- Same as the rasterizer output.

CPU cost:

- Lower after first run.

GPU cost:

- Same as atlas rendering.

Memory usage:

- Same runtime memory; extra disk space.

Startup cost:

- Lower if cache hits.

Runtime cost:

- Low.

Scaling and DPI:

- Requires separate cache entries.

Unicode support:

- Good if dynamically populated.

Box-drawing and terminal compatibility:

- Good; procedural symbols need no cache.

Implementation complexity:

- Medium.

Maintainability:

- Medium; invalidation must be rigorous.

Advantages:

- Faster startup.
- Avoids repeated expensive rasterization.
- Good for stable user fonts.

Disadvantages:

- Does not inherently improve crispness.
- Can produce stale or incorrect output if invalidation is weak.
- Adds file-format maintenance.

Comparison with pre-rasterized atlas:

- It is a persistence layer for the current approach.
- It should not replace the live renderer.

Terminal suitability:

- Good after correctness is finished.

Rating: 8/10.

## Cross-Cutting Issues

### Shaping

Text shaping converts Unicode text into glyph IDs and positions. HarfBuzz is a standard shaping engine used across browsers, UI systems, editors, and operating systems. Skia's shaped text design also separates shaping from drawing: produce shaped glyph runs first, then let the renderer consume glyph IDs and positions.

For Forge:

- ASCII terminal text does not need complex shaping.
- Ligatures need shaping.
- Arabic, Indic scripts, Thai, emoji ZWJ sequences, variation selectors, and combining marks need shaping/fallback.
- A terminal can start with simple shaping for monospace Latin but should eventually use HarfBuzz or a comparable shaper.

### Font Fallback

Unicode support is not just about atlas size. It requires selecting fallback fonts when the primary font lacks a glyph.

For Forge:

- Fallback needs to preserve cell metrics.
- Emoji and Nerd Font symbols may have different metrics.
- Wide characters need correct cell width handling.
- Fallback atlas entries must include face identity and glyph ID.

### DPI and Resizing

Correct DPI behavior requires:

- Rasterizing at physical pixel size.
- Rebuilding or selecting the correct atlas on scale factor changes.
- Pixel-snapping glyph origins.
- Avoiding bitmap scaling.
- Separating cell layout stretch from glyph bitmap size.

### Box Drawing

Box drawing should not rely entirely on font glyphs. Fonts vary too much. Procedural rendering is usually better for:

- Horizontal and vertical lines.
- Corners and junctions.
- Block elements.
- Braille.
- Powerline separators.

The parser must also support terminal character set modes such as DEC special graphics, because applications can emit legacy ACS bytes instead of Unicode box characters.

## Comparison Against Forge's Current Atlas

| Technique | Beats Current Atlas At | Loses To Current Atlas At | Replace Atlas? |
|---|---|---|---|
| Bitmap fonts | Single-size sharpness, simplicity | User fonts, Unicode, scaling | No |
| Runtime rasterization | Flexibility, dynamic glyphs | Needs cache discipline | No, feed atlas |
| Dynamic atlas | Startup, Unicode scalability | More complexity | Evolve toward it |
| FreeType/CoreText/DirectWrite | Raster quality, metrics, shaping ecosystem | Dependency/platform complexity | No, use as backend |
| Grayscale AA | Portable quality | Slight softness at low DPI | Already part of atlas |
| Hinting | Small text clarity | Metric complexity | Add/improve before atlas |
| LCD/subpixel | Low-DPI sharpness | Transparency, display assumptions | Optional only |
| CPU compositing | Simpler debug/fallback | CPU bandwidth | No |
| GPU instancing | Upload/CPU efficiency | Pipeline complexity | Add later |
| SDF | Scalable text | Small terminal crispness | No |
| MSDF | Scalable corners | Small terminal crispness, complexity | No |
| Vector path | Resolution independence | Complexity and cost | No |
| Hybrid | Broad UI flexibility | Multiple render paths | Later only |
| Procedural symbols | Terminal graphics correctness | General text | Use alongside atlas |
| Persistent cache | Startup speed | Invalidation complexity | Add later |

## Final Awards

Best overall technique:

- Dynamic rasterized glyph atlas with high-quality rasterization, shaping, fallback, and procedural terminal symbols.

Best low-resource technique:

- Static bitmap fonts or a small pre-rasterized atlas. Good for embedded targets, not best for Forge's default user-font model.

Best text-quality technique:

- Native-quality rasterization using FreeType/CoreText/DirectWrite-style engines with hinting, grayscale AA or optional LCD, shaped by HarfBuzz or platform shaping, then cached in an atlas.

Best practical production-ready technique:

- Rasterized glyph atlas with dynamic glyph insertion and robust cache keys.

Best technique for extremely crisp terminal text:

- Target-size hinted grayscale glyph atlas, pixel-snapped glyph quads, no post-rasterization scaling, procedural box drawing/block/braille/powerline symbols.

Best future performance optimization:

- GPU instanced glyph rendering using the same atlas.

Worst tempting replacement:

- MSDF/SDF as the main terminal text renderer. It sounds advanced, but it is usually worse than hinted rasterization for small dense terminal text.

## Final Recommendation For Forge

Keep and improve the current pre-rasterized glyph atlas approach.

Do not replace it with SDF, MSDF, or GPU vector text for normal terminal text. Those approaches solve different problems: continuous scaling, 3D labels, large UI display text, and vector graphics. Forge's central problem is dense, small, fixed-size, high-contrast terminal text. Native-size rasterized glyphs in an atlas are the right foundation.

Recommended roadmap:

1. Keep glyphs rasterized at exact physical pixel size.
2. Preserve nearest or appropriate non-blurring sampling for coverage masks.
3. Keep glyph origins integer pixel-snapped.
4. Maintain separation between cell geometry scaling and glyph bitmap scaling.
5. Improve the rasterization backend and metrics handling.
6. Add HarfBuzz or another shaping path for ligatures and complex scripts.
7. Add real fallback font resolution and atlas keys.
8. Keep procedural rendering for box drawing, block elements, braille, and powerline.
9. Add per-DPI/per-size atlas invalidation.
10. Convert fixed preloading into a dynamic atlas with ASCII/common glyph prewarm.
11. Add a persistent on-disk glyph cache only after the runtime cache keys are correct.
12. Consider GPU instanced glyph rendering later to reduce vertex bandwidth.

If Forge implements the above, the current atlas architecture can reach professional terminal quality without taking on unnecessary rendering complexity.

## Sources

- FreeType Glyph Conventions: https://freetype.org/freetype2/docs/glyphs/glyphs-3.html
- FreeType Glyph Retrieval API: https://freetype.org/freetype2/docs/reference/ft2-glyph_retrieval.html
- HarfBuzz manual: https://harfbuzz.github.io/what-is-harfbuzz.html
- Microsoft DirectWrite overview: https://learn.microsoft.com/en-us/windows/win32/directwrite/direct-write-portal
- Skia shaped text design: https://skia.org/docs/dev/design/text_shaper/
- msdfgen project: https://github.com/Chlumsky/msdfgen
