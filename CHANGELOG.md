# Changelog

Changes are recorded here as the project moves toward its first release. No public Guion release has been published.

## Unreleased — pre-release development

- Added pure `std:time` local-time resolution that reports exact times, daylight-saving gaps, and folds with all candidate instants using bundled tzdata 2026c. Calendar events now preserve local endpoint identity and ambiguity instead of selecting an instant; `guion --version` reports the timezone-data version.
- Renamed active product surfaces from Veritas to Guion: CLI, Cargo packages, source and artifact extensions, project filenames, examples, standard library, compiler seed, and packaging tools.
- Added transition reads for selected legacy Veritas manifests, locks, project state, source files, and existing artifacts.
- Added a Guion-named acceptance-report cohort for nine retained applications; the frozen Veritas evidence remains unchanged.
- Replaced current-facing getting-started, architecture, reference, compatibility, and status documentation; preserved the Veritas-era checkpoints and results as history.
- Added an agent start guide, four runnable recipes, a versioned machine index, and CLI JSON report envelope schemas.
- Full Guion release-matrix, benchmark, evidence-index, and pre-release handoff checks remain open.
