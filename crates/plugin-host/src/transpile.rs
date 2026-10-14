//! Turns TypeScript into JavaScript by removing types (no type checking).

use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use std::path::{Path, PathBuf};

/// Returns JavaScript for `source`. `.js`/`.mjs` files are returned unchanged.
pub fn to_javascript(path: &Path, source: &str) -> Result<String, String> {
    transpile(path, source).map(|(code, _)| code)
}

/// Where each position of generated JavaScript came from in the TypeScript
/// source. The generated code is re-printed (comments and blank lines are
/// gone), so without this, line numbers in stack traces would not match the file.
#[derive(Debug, Clone, Default)]
pub(crate) struct LineMap {
    /// Per generated line (0-based): `(generated column, source line, source column)`, sorted by column.
    lines: Vec<Vec<(u32, u32, u32)>>,
}

impl LineMap {
    /// Maps a 1-based generated `line:column` to the 1-based source position.
    pub(crate) fn lookup(&self, line: u32, column: u32) -> Option<(u32, u32)> {
        let tokens = self.lines.get(line.checked_sub(1)? as usize)?;
        let column = column.saturating_sub(1);
        let index = tokens.partition_point(|(c, _, _)| *c <= column);
        let (_, src_line, src_col) = tokens.get(index.saturating_sub(1))?;
        Some((src_line + 1, src_col + 1))
    }
}

/// Like [`to_javascript`], plus a [`LineMap`] for TypeScript files.
pub(crate) fn transpile(path: &Path, source: &str) -> Result<(String, Option<LineMap>), String> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(extension, "ts" | "mts" | "cts" | "tsx") {
        return Ok((source.to_string(), None));
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
    let options = CodegenOptions { source_map_path: Some(PathBuf::from(path)), ..CodegenOptions::default() };
    let output = Codegen::new().with_options(options).build(&program);
    let map = output.map.map(|map| {
        let mut lines: Vec<Vec<(u32, u32, u32)>> = Vec::new();
        for token in map.get_tokens() {
            let line = token.get_dst_line() as usize;
            if lines.len() <= line {
                lines.resize_with(line + 1, Vec::new);
            }
            lines[line].push((token.get_dst_col(), token.get_src_line(), token.get_src_col()));
        }
        lines.iter_mut().for_each(|tokens| tokens.sort_unstable_by_key(|t| t.0));
        LineMap { lines }
    });
    Ok((output.code, map))
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
    fn generated_positions_map_back_to_the_typescript_source() {
        let source = "// A comment that codegen drops.\n\ninterface A { a: string }\n\nexport function f(a: number): string {\n  throw new Error(`bad ${a}`);\n}\n";
        let (js, map) = transpile(Path::new("index.ts"), source).unwrap();
        let map = map.expect("a line map for TypeScript");
        let (line, column) = js.lines().enumerate().find_map(|(i, l)| l.find("throw").map(|c| (i as u32 + 1, c as u32 + 1))).unwrap();
        assert_eq!(map.lookup(line, column), Some((6, 3)), "{js}");
        assert_eq!(map.lookup(999, 1), None);
    }

    #[test]
    fn tsx_is_transpiled() {
        let js = to_javascript(Path::new("view.tsx"), "export const n: number = 1;").unwrap();
        assert!(js.contains("export const n = 1"), "{js}");
    }

    #[test]
    fn javascript_is_unchanged() {
        assert_eq!(to_javascript(Path::new("a.js"), "let a: x").unwrap(), "let a: x");
    }
}
