#[test]
fn test_shaper() {
    let font_data = std::fs::read("/usr/share/fonts/OTF/FiraCode-Regular.otf").or_else(|_| std::fs::read("/usr/share/fonts/TTF/FiraCode-Regular.ttf")).unwrap();
    let face = rustybuzz::Face::from_slice(&font_data, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str("!==");
    buffer.guess_segment_properties();
    
    let glyph_buffer = rustybuzz::shape(&face, &[], buffer);
    let infos = glyph_buffer.glyph_infos();
    println!("Glyphs for '!==': {}", infos.len());
    for info in infos {
        println!("glyph_id: {}", info.glyph_id);
    }
}
