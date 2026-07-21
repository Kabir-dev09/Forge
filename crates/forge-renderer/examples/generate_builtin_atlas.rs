use forge_renderer::font::{atlas::GlyphAtlas, builtin, rasterizer::FontRasterizer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let size = builtin::DEFAULT_SIZE;
    let regular = FontRasterizer::from_bytes(builtin::regular(), size)?;
    let bold = FontRasterizer::from_bytes(builtin::bold(), size)?;
    let italic = FontRasterizer::from_bytes(builtin::italic(), size)?;
    let bold_italic = FontRasterizer::from_bytes(builtin::bold_italic(), size)?;
    let atlas = GlyphAtlas::build(
        &regular,
        Some(&bold),
        Some(&italic),
        Some(&bold_italic),
        size,
        false,
    )?;
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fonts/JetBrainsMono-14.fga");
    std::fs::write(&output, atlas.to_cache_bytes())?;
    println!("wrote {}", output.display());
    Ok(())
}
