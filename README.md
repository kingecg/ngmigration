# angular-migrator

A Rust CLI and library that migrates Angular applications across multiple major
versions. Given a project and a target Angular major, it plans and applies the
required dependency bumps, configuration changes, and (optionally) structural
code rewrites for every intermediate major version.

## Why

Angular releases a new major version roughly every six months. Upgrading from an
old version (e.g. 12) to a recent one (e.g. 17) requires stepping through each
intermediate major and manually applying deprecations that accumulate over time.
`angular-migrator` encodes those per-major changes in a data-driven rule catalog
so the process can be planned, reviewed, and applied offline.

## Commands

```
angular-migrator analyze <PATH>
    Detect the project and report its Angular / CLI / toolchain versions.

angular-migrator plan <PATH> --target <MAJOR> [--report <FILE>]
    Print the migration path and the full list of changes for each major step.
    Optionally write a Markdown report.

angular-migrator migrate <PATH> --target <MAJOR> [--dry-run] [--apply-control-flow]
                          [--apply-recommended] [--offline] [--report <FILE>]
    Apply the migration in place. Prints every applied change and warnings.

angular-migrator catalog
    Print the supported version catalog (majors 6-22).
```

### Options

| Flag | Meaning |
| --- | --- |
| `--target <MAJOR>` | Destination Angular major (2-22). Must be higher than the current major. |
| `--dry-run` | Compute everything, write nothing. |
| `--apply-control-flow` | Also rewrite `*ngIf` / `*ngFor` templates to `@if` / `@for` (v16->17 step). Off by default; occurrences that cannot be rewritten safely are reported as notes. |
| `--apply-recommended` | Apply third-party version suggestions (e.g. bump `@ngrx/store` to match Angular's major) instead of only reporting them. |
| `--offline` | Skip npm registry lookups; use only the built-in local compatibility database. |
| `--report <FILE>` | Write a Markdown report of the plan or migration. |

## What it does

For each major transition, the tool can apply:

- **Dependency updates** — bump `@angular/*`, `@angular-devkit/*`, `rxjs`,
  `zone.js`, `typescript`, `tslib`, etc. to the highest stable version known to
  work with the target major.
- **Dependency removal** — e.g. `@angular/language-service` (v16), `protractor`
  (v17), `@angular-devkit/build-ng-packagr`.
- **Config changes** — remove `enableIvy` from `angularCompilerOptions` (v13),
  remove `entryComponents` from `@NgModule` (v14), remove the `e2e` architect
  target from `angular.json` (v17), manage other compiler options.
- **Script cleanup** — strip deprecated CLI flags (`--prod`, `--aot`, ...) from
  npm scripts (v13), remove the `e2e` script (v17).
- **Control-flow rewrite** *(opt-in)* — `*ngIf` / `*ngFor` -> `@if` / `@for`
  with per-element re-indentation. Only applied at the v16->v17 step.
- **Third-party compatibility** — a curated local database of libraries that
  track the Angular major (`@ngrx/*`, `ng-zorro-antd`, `primeng`,
  `ngx-toastr`), optionally augmented by live npm `peerDependencies` checks.

The migration path always traverses **every** intermediate major (e.g.
12 -> 13 -> ... -> 17), mirroring the official `ng update` behavior.

## Design notes

- **Offline-first.** All core behavior works without a network connection.
  npm registry lookups are gated behind the `network` cargo feature (enabled by
  default) and skipped with `--offline`.
- **Conservative source edits.** TS/HTML files are edited with string/comment
  awareness: property removal touches only top-level `@NgModule` object members
  and skips strings and comments; control-flow rewrites bail out on
  micro-syntax (`else` / `then` / aliases / index vars) instead of guessing.
- **JSON formatting.** `package.json`, `angular.json`, and `tsconfig.json` are
  re-serialized on change; original formatting and comments (JSONC) are
  tolerated on read but not preserved on write.
- **Version catalog.** Per-major latest stable versions for majors 6-20 were
  verified against the npm registry. Majors 21-22 are estimates and are flagged
  as such in the catalog output.

## Version catalog

| Major | `@angular/core` | `typescript` | Notes |
| --- | --- | --- | --- |
| 6-20 | per-major latest stable (verified) | newest TS supported by that major | Ivy enabled from 9 |
| 21-22 | estimated | - | marked `confirmed: false` |

## Library usage

The crate exposes a library API in addition to the CLI:

- `catalog`, `plan`, `migrate` modules for programmatic migration.
- `model::{MigrationPlan, MigrationOutcome, MajorStep}` for inspecting results.

## Roadmap

- Support for `ng update`-style migrations for more third-party libraries.
- Preserving JSON formatting/comments on rewrite.
- Applying `@if`/`@for` rewrites at v18+ steps for deferred `@defer` blocks.

## License

MIT
