use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/shaders/grid.vert.glsl");
    println!("cargo:rerun-if-changed=src/shaders/grid.frag.glsl");

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let compiler = shaderc::Compiler::new().expect("Failed to initialize shader compiler");

    let mut options = shaderc::CompileOptions::new().unwrap();
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);

    let shaders = [
        (
            "src/shaders/grid.vert.glsl",
            shaderc::ShaderKind::Vertex,
            "grid.vert.spv",
        ),
        (
            "src/shaders/grid.frag.glsl",
            shaderc::ShaderKind::Fragment,
            "grid.frag.spv",
        ),
    ];

    for (source_path, kind, output_name) in shaders.iter() {
        let source_text = fs::read_to_string(source_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", source_path, e));

        let compiled = compiler
            .compile_into_spirv(&source_text, *kind, source_path, "main", Some(&options))
            .unwrap_or_else(|e| panic!("Failed to compile {}: {}", source_path, e));

        let dest_path = Path::new(&out_dir).join(output_name);
        fs::write(&dest_path, compiled.as_binary_u8())
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", dest_path.display(), e));
    }
}
