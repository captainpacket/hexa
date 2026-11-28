//! `import("x")` is an import wherever it is written, in TypeScript and
//! JavaScript alike. A dependency does not stop being one because it sits
//! inside a function, a class or an export.

use std::path::Path;

use hexa_analysis::domain::Language;

fn paths(file: &str, language: Language, source: &str) -> Vec<String> {
    let imports = hexa_analysis::default_ast()
        .extract_imports(Path::new(file), source, language)
        .expect("the source parses");
    imports.into_iter().map(|i| i.raw_path).collect()
}

const P: &str = "../adapters/secondary/a.js";

/// Every place the report found unflagged, plus the two that already worked.
const SHAPES: &[(&str, &str)] = &[
    ("exported async function", "export async function load() { return import('@P'); }"),
    ("await inside a function", "export async function load() { const m = await import('@P'); return m; }"),
    ("exported const", "export const p = import('@P');"),
    ("plain function", "function load() { return import('@P'); }\nexport { load };"),
    ("class method", "class L { load() { return import('@P'); } }\nexport default L;"),
    ("default export", "export default () => import('@P');"),
    ("inside an if", "if (globalThis.x) { import('@P'); }"),
    ("top-level const", "const p = import('@P');"),
    ("top-level then", "import('@P').then(function () {});"),
    ("template string with no substitution", "export const p = () => import(`@P`);"),
];

#[test]
fn a_dynamic_import_is_an_edge_wherever_it_is_written() {
    let mut wrong = Vec::new();
    for (file, language) in [
        ("src/domain/x.ts", Language::TypeScript),
        ("src/domain/x.js", Language::TypeScript),
        ("src/domain/x.mjs", Language::TypeScript),
    ] {
        for (shape, source) in SHAPES {
            let found = paths(file, language, &source.replace("@P", P));
            if found != [P] {
                wrong.push(format!("{file}, {shape}: {found:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "each shape imports exactly {P}:\n{}", wrong.join("\n"));
}

#[test]
fn a_specifier_that_names_no_one_file_is_not_guessed_at() {
    for source in [
        "export const f = (name) => import(name);",
        "export const f = (name) => import('./locales/' + name);",
        "export const f = (name) => import(`./locales/${name}.js`);",
    ] {
        let found = paths("src/domain/x.js", Language::TypeScript, source);
        assert!(found.is_empty(), "{source}: {found:?}");
    }
}
