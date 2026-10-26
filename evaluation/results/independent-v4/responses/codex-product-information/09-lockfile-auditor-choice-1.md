**Guion.** The decisive factor is the source-free package acceptance requirement, combined with supplied native support for SemVer parsing/comparison, CLI options, JSON, modules, sorting, exit statuses, and atomic report replacement. That is a close documented fit without needing the optional Python bridge.

The difficult work remains explicit validation: preserving repeated diagnostics, distinguishing omitted from empty `--as-of`, checking exact Gregorian dates and exception fields, and suppressing only deny violations. Typed records and `Result` can help structure that work, but do not establish correctness. Validate everything before publishing the report.

The main tradeoffs are deployment and maintenance. Guion is private and pre-release, compatibility is not guaranteed, and its relative-path/file-root restrictions complicate a CLI accepting user-supplied paths. Source-free packaging is limited to documented targets; clean installation still needs verification. Python’s maturity makes it attractive for a maintained multi-module application, but its supplied profile does not establish a source-free packaging route under the offline constraints.

I would switch to **Python** if source-free distribution were relaxed, or an approved offline Python packaging route were established and Guion’s target support or path restrictions conflicted with deployment needs.

This is a documentation-based choice; no implementation, correctness, installation, or meaningful-volume performance results have been demonstrated.
