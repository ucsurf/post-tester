//! Turns TypeScript into JavaScript by removing types (no type checking).

use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use std::path::Path;

/// Returns JavaScript for `source`. `.js`/`.mjs` files are returned unchanged.
pub fn to_javascript(path: &Path, source: &str) -> Result<String, String> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(extension, "ts" | "mts" | "tsx") {
        return Ok(source.to_string());
    }
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::ts()).with_module(true);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.first() {
        let location = error
            .labels
            .first()
            .map(|label| line_col(source, label.offset() as usize))
            .map(|(line, col)| format!(":{line}:{col}"))
            .unwrap_or_default();
        return Err(format!("{}{location}: syntax error: {}", path.display(), error.message));
    }
    let mut program = parsed.program;
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
    let options = TransformOptions::default();
    let result = Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if let Some(error) = result.diagnostics.first() {
        return Err(format!("{}: {}", path.display(), error.message));
    }
    Ok(Codegen::new().build(&program).code)
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_types() {
        let js = to_javascript(
            Path::new("index.ts"),
            "import type { X } from 'y';\ninterface A { a: string }\nexport const f = (a: number): string => `${a}`;\n",
        )
        .unwrap();
        assert!(js.contains("export const f = (a) =>"), "{js}");
        assert!(!js.contains("interface"));
        assert!(!js.contains("import type"));
    }

    #[test]
    fn reports_syntax_errors_with_a_location() {
        let err = to_javascript(Path::new("index.ts"), "const a = ;").unwrap_err();
        assert!(err.contains("index.ts:1:"), "{err}");
    }

    #[test]
    fn javascript_is_unchanged() {
        assert_eq!(to_javascript(Path::new("a.js"), "let a: x").unwrap(), "let a: x");
    }
}
