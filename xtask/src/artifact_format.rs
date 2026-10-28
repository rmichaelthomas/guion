use std::fs;
use std::path::{Path, PathBuf};

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{BinOp, Expr, ExprBinary, ExprField, ExprIndex, ExprLit, Lit, Macro, Token};

pub fn audit_repository(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files)?;
    collect_rust_files(&root.join("xtask"), &mut files)?;
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let assertions = hard_coded_format_assertions(&source)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for line in assertions {
            violations.push(format!(
                "{relative}:{line}: artifact format assertion uses a numeric literal; compare against guion_model::Artifact::CURRENT_VERSION"
            ));
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "stale artifact-format test expectations found (current format {}):\n{}",
            guion_model::Artifact::CURRENT_VERSION,
            violations.join("\n")
        ))
    }
}

fn hard_coded_format_assertions(source: &str) -> Result<Vec<usize>, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut visitor = FormatAssertionVisitor { lines: Vec::new() };
    visitor.visit_file(&file);
    Ok(visitor.lines)
}

struct FormatAssertionVisitor {
    lines: Vec<usize>,
}

impl<'ast> Visit<'ast> for FormatAssertionVisitor {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        let stale = if mac.path.is_ident("assert_eq") || mac.path.is_ident("assert_ne") {
            has_numeric_format_comparison(mac)
        } else if mac.path.is_ident("assert") {
            has_numeric_format_assertion(mac)
        } else {
            false
        };
        if stale {
            self.lines.push(mac.span().start().line);
        }
        visit::visit_macro(self, mac);
    }
}

fn has_numeric_format_comparison(mac: &Macro) -> bool {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(arguments) = parser.parse2(mac.tokens.clone()) else {
        return false;
    };
    if arguments.len() < 2 {
        return false;
    }
    let mut arguments = arguments.iter();
    let first = arguments.next().expect("at least two arguments");
    let second = arguments.next().expect("at least two arguments");
    (is_artifact_format_expression(first) && is_integer_literal(second))
        || (is_artifact_format_expression(second) && is_integer_literal(first))
}

fn has_numeric_format_assertion(mac: &Macro) -> bool {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(arguments) = parser.parse2(mac.tokens.clone()) else {
        return false;
    };
    let Some(condition) = arguments.first() else {
        return false;
    };
    let mut visitor = FormatComparisonVisitor { found: false };
    visitor.visit_expr(condition);
    visitor.found
}

struct FormatComparisonVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for FormatComparisonVisitor {
    fn visit_expr_binary(&mut self, binary: &'ast ExprBinary) {
        if matches!(binary.op, BinOp::Eq(_) | BinOp::Ne(_))
            && ((is_artifact_format_expression(&binary.left) && is_integer_literal(&binary.right))
                || (is_artifact_format_expression(&binary.right)
                    && is_integer_literal(&binary.left)))
        {
            self.found = true;
        }
        visit::visit_expr_binary(self, binary);
    }
}

fn is_artifact_format_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Field(ExprField { member, base, .. }) => {
            matches!(member, syn::Member::Named(name) if name == "format_version")
                && !root_identifier(base).is_some_and(|name| name == "lock" || name == "manifest")
        }
        Expr::Index(ExprIndex { index, .. }) => {
            matches!(index.as_ref(), Expr::Lit(ExprLit { lit: Lit::Str(name), .. }) if name.value() == "artifact_format")
        }
        Expr::Paren(expression) => is_artifact_format_expression(&expression.expr),
        Expr::Group(expression) => is_artifact_format_expression(&expression.expr),
        _ => false,
    }
}

fn root_identifier(expression: &Expr) -> Option<&syn::Ident> {
    match expression {
        Expr::Path(path) if path.qself.is_none() => path.path.get_ident(),
        Expr::Field(field) => root_identifier(&field.base),
        Expr::Index(index) => root_identifier(&index.expr),
        Expr::Paren(expression) => root_identifier(&expression.expr),
        Expr::Group(expression) => root_identifier(&expression.expr),
        _ => None,
    }
}

fn is_integer_literal(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::Lit(ExprLit {
            lit: Lit::Int(_),
            ..
        })
    )
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "read test source directory {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read {} entry: {error}", directory.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", path.display()))?;
        if kind.is_dir() {
            collect_rust_files(&path, files)?;
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{audit_repository, hard_coded_format_assertions};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn identifies_hard_coded_artifact_format_expectations() {
        let source = r#"
            fn test() {
                assert_eq!(artifact.format_version, 39);
                assert_eq!(report["artifact_format"], 40);
                assert!(artifact.format_version == 41);
            }
        "#;

        assert_eq!(hard_coded_format_assertions(source).unwrap(), [3, 4, 5]);
    }

    #[test]
    fn ignores_current_version_constants_and_compatibility_setup() {
        let source = r#"
            fn test() {
                assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
                legacy.format_version = 40;
                assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
                assert_eq!(lock["format_version"], 2);
            }
        "#;

        assert!(hard_coded_format_assertions(source).unwrap().is_empty());
    }

    #[test]
    fn ignores_fake_assertions_inside_comments_and_string_literals() {
        let source = r##"
            // assert_eq!(artifact.format_version, 39);
            fn test() {
                let example = r#"assert_eq!(artifact.format_version, 39);"#;
            }
        "##;

        assert!(hard_coded_format_assertions(source).unwrap().is_empty());
    }

    #[test]
    fn repository_audit_names_the_stale_test_before_any_gate_runs() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("guion-format-audit-{}-{nonce}", std::process::id()));
        let source_path = root.join("crates/runtime/tests/stale_format.rs");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        // The audit scans both `crates/` and `xtask/`, and requires each directory to exist.
        fs::create_dir_all(root.join("xtask")).unwrap();
        fs::write(
            &source_path,
            "#[test] fn stale() { assert_eq!(artifact.format_version, 39); }",
        )
        .unwrap();

        let result = audit_repository(&root);

        fs::remove_dir_all(&root).unwrap();
        let error = result.expect_err("the stale assertion must block the fast preflight");
        assert!(error.contains("crates/runtime/tests/stale_format.rs:1"));
        assert!(error.contains("CURRENT_VERSION"));
    }
}
