use std::path::Path;

/// Returns whether a changed path represents product or shipped-document input.
/// Completion output, its working ledger, and implementation plans describe the
/// verification process and do not alter the packaged product.
pub fn include_in_product_snapshot(path: &Path) -> bool {
    !path.starts_with("evaluation/completion/results")
        && path != Path::new("evaluation/completion/status.json")
        && !path.starts_with("docs/superpowers/plans")
        && path != Path::new("docs/completion-report.md")
}

#[cfg(test)]
mod tests {
    use super::include_in_product_snapshot;
    use std::path::Path;

    #[test]
    fn excludes_completion_metadata_and_keeps_product_inputs() {
        for path in [
            "evaluation/completion/results/release.json",
            "evaluation/completion/status.json",
            "docs/superpowers/plans/2026-10-24-veritas-complete-product.md",
            "docs/completion-report.md",
        ] {
            assert!(
                !include_in_product_snapshot(Path::new(path)),
                "completion metadata unexpectedly affects the product snapshot: {path}"
            );
        }
        for path in ["crates/runtime/src/lib.rs", "README.md", "docs/reference.md"] {
            assert!(
                include_in_product_snapshot(Path::new(path)),
                "product input was unexpectedly excluded from the snapshot: {path}"
            );
        }
    }
}
