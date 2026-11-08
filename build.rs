//! Minifies the web scoreboard's `app.js` at build time; the binary
//! embeds and serves the result (see `src/web/mod.rs`).

use std::path::PathBuf;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_minifier::{Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;

const SOURCE: &str = "assets/web/app.js";

fn main() {
    println!("cargo:rerun-if-changed={SOURCE}");
    let src = std::fs::read_to_string(SOURCE).expect("read app.js");

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &src, SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        for e in parsed.diagnostics {
            println!("cargo:warning={SOURCE}: {e}");
        }
        panic!("{SOURCE} has syntax errors");
    }
    let mut program = parsed.program;
    let minified = Minifier::new(MinifierOptions::default()).minify(&allocator, &mut program);
    let code = Codegen::new()
        .with_options(CodegenOptions::minify())
        .with_scoping(minified.scoping)
        .build(&program)
        .code;

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("app.min.js");
    std::fs::write(out, code).expect("write app.min.js");
}
