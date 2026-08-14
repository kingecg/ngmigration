//! Data-driven migration rules for each Angular major transition.
//!
//! Rules are keyed by the transition `from -> from + 1` and applied in order
//! by [`crate::migrate`]. Version alignment of `@angular/*` and the toolchain
//! (`typescript`, `rxjs`, `zone.js`, `tslib`) happens once at the end against
//! the target major's catalog entry, so this module only encodes *structural*
//! changes (source rewrites, config changes, removed packages, notes).

use crate::model::MigrationRule;

/// Options that influence rule generation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanOptions {
    /// Also emit the structural-directive -> control-flow migration rule
    /// (Angular 17+). Defaults to a note instead.
    pub apply_control_flow: bool,
}

const V9_IVY_NOTE: &str = "Angular 9 enables Ivy by default. Projects still on View Engine \
    (angularCompilerOptions.enableIvy: false) must be migrated to Ivy before continuing; \
    this tool removes the flag at Angular 13 where View Engine is deleted.";

/// Rules for upgrading one major step (`from` -> `from + 1`).
pub fn step_rules(from: u32, opts: PlanOptions) -> Vec<MigrationRule> {
    let mut rules: Vec<MigrationRule> = Vec::new();
    match from {
        6 => {
            rules.push(MigrationRule::note(
                "Angular 7: TypeScript 3.1 required. No automated source changes. See the \
                 official Angular 7 update guide for manual steps.",
            ));
        }
        7 => {
            rules.push(MigrationRule::note(
                "Angular 8: TypeScript 3.4, differential loading enabled by default. \
                 `@angular/http` is fully removed (use `@angular/common/http`).",
            ));
        }
        8 => {
            rules.push(MigrationRule::note(
                "Angular 9 (Ivy default): recompile all libraries; View Engine libraries must \
                 publish Ivy-compatible (partial-Ivy) builds.",
            ));
            rules.push(MigrationRule::note(V9_IVY_NOTE));
            rules.push(MigrationRule::note(
                "@ViewChild / @ContentChild: in Ivy the `static` flag defaults to false. Run the \
                 official schematic (`ng update @angular/core --from 8 --to 9`) which inserts \
                 `static: true` where the query result is used before the view initialises. This \
                 tool does not automate that analysis.",
            ));
            rules.push(MigrationRule::note(
                "TypeScript 3.6-3.8, Node 10.13+, tslib ^2.0.0 required (bumped by the tool).",
            ));
        }
        9 => {
            rules.push(MigrationRule::note(
                "Angular 10: TypeScript 3.9; IE9/IE10 support dropped from the default \
                 browser targets.",
            ));
        }
        10 => {
            rules.push(MigrationRule::note(
                "Angular 11: TypeScript 4.0; browser support for Firefox ESR unchanged.",
            ));
        }
        11 => {
            rules.push(MigrationRule::note(
                "Angular 12: TypeScript 4.2, Webpack 5, IE11 deprecation announced. \
                 `--prod` and related build flags become deprecated.",
            ));
        }
        12 => {
            rules.push(MigrationRule::RemoveCompilerOption {
                key: "enableIvy".into(),
            });
            rules.push(MigrationRule::StripScriptFlags {
                flags: vec![
                    "--prod".into(),
                    "--aot".into(),
                    "--build-optimizer".into(),
                    "--vendor-chunk".into(),
                    "--common-chunk".into(),
                    "--named-chunks".into(),
                    "--output-hashing".into(),
                    "--extract-licenses".into(),
                    "--show-circular-dependencies".into(),
                ],
            });
            rules.push(MigrationRule::DepRemove {
                package: "@angular-devkit/build-ng-packagr".into(),
            });
            rules.push(MigrationRule::note(
                "Angular 13 removes View Engine entirely: `enableIvy` is deleted from \
                 angularCompilerOptions (removed by the tool). Verify the project is Ivy-ready.",
            ));
            rules.push(MigrationRule::note(
                "Angular 13 removes the deprecated `Renderer` type (use `Renderer2`), removes \
                 the `--prod`/`--aot`/etc. build flags (stripped from npm scripts by the tool), \
                 and drops `entryComponents` support.",
            ));
            rules.push(MigrationRule::note(
                "rxjs 6 -> 7: the tool bumps rxjs to ^7.5.7. Manual work may be required: \
                 `toPromise` removed, `combineLatest`/`merge`/`forkJoin` signatures changed, \
                 `switchMap`/`map` untouched.",
            ));
            rules.push(MigrationRule::note(
                "TypeScript 4.4-4.6, Node 12.20+ required.",
            ));
        }
        13 => {
            rules.push(MigrationRule::RemoveNgModuleField {
                field: "entryComponents".into(),
            });
            rules.push(MigrationRule::note(
                "Angular 14 removes `entryComponents` from @NgModule (removed by the tool). \
                 Forms are now strictly typed; untyped forms may require `UntypedFormControl` \
                 etc. when strict templates are enabled.",
            ));
            rules.push(MigrationRule::note(
                "TypeScript 4.6-4.8, Node 14.15+ required.",
            ));
        }
        14 => {
            rules.push(MigrationRule::note(
                "Angular 15 removes `ComponentFactoryResolver`, `ComponentFactory` and the \
                 factory-based `ViewContainerRef.createComponent` overload. Replace \
                 `resolver.resolveComponentFactory(X)` with \
                 `viewContainerRef.createComponent(X)`. This tool does not automate that rewrite.",
            ));
            rules.push(MigrationRule::note(
                "Standalone components are now stable; `provideRouter`/`bootstrapApplication` \
                 are recommended for new code.",
            ));
            rules.push(MigrationRule::note(
                "TypeScript 4.8-5.0, Node 14.20+/16.13+ required.",
            ));
        }
        15 => {
            rules.push(MigrationRule::DepRemove {
                package: "@angular/language-service".into(),
            });
            rules.push(MigrationRule::note(
                "Angular 16: the standalone `@angular/language-service` package is removed \
                 (removed by the tool; language support ships inside @angular/compiler-cli).",
            ));
            rules.push(MigrationRule::note(
                "Standalone APIs are now the default recommendation; `ng new` generates \
                 standalone apps.",
            ));
            rules.push(MigrationRule::note(
                "TypeScript 4.9-5.1, Node 16.14+/18.10+ required.",
            ));
        }
        16 => {
            rules.push(MigrationRule::RemoveWorkspaceTarget {
                target: "e2e".into(),
            });
            rules.push(MigrationRule::RemoveScript {
                script: "e2e".into(),
            });
            rules.push(MigrationRule::DepRemove {
                package: "protractor".into(),
            });
            if opts.apply_control_flow {
                rules.push(MigrationRule::ControlFlowMigration);
            } else {
                rules.push(MigrationRule::note(
                    "Angular 17 introduces `@if`/`@for`/`@switch` control-flow blocks. Run \
                     `ng generate @angular/core:control-flow` (or re-run this tool with \
                     `--apply-control-flow`) to migrate structural directives. The tool's \
                     opt-in rewrite handles plain `*ngIf`/`*ngFor` on elements and ng-container.",
                ));
            }
            rules.push(MigrationRule::note(
                "Angular 17 removes `ng e2e` and the protractor builder (e2e target, script and \
                 dependency removed by the tool); migrate e2e tests to Playwright or another \
                 runner.",
            ));
            rules.push(MigrationRule::note(
                "Vite is the new default build/dev server; esbuild-powered. SSR uses \
                 `@angular/ssr` instead of `@nguniversal/*`.",
            ));
            rules.push(MigrationRule::note("Node 18.13+/20.9+ required."));
        }
        17 => {
            rules.push(MigrationRule::note(
                "Angular 18: optional `@defer` blocks for lazy-loading template parts; \
                 zoneless change detection in developer preview.",
            ));
            rules.push(MigrationRule::note("Node 18.19+/20.11+ required."));
        }
        18 => {
            rules.push(MigrationRule::note(
                "Angular 19: `provideAppInitializer` replaces the deprecated \
                 `APP_INITIALIZER` multi-provider; standalone components are the default for \
                 `ng new`.",
            ));
            rules.push(MigrationRule::note("Node 20.11+/22.11+ required."));
        }
        19 => {
            rules.push(MigrationRule::note(
                "Angular 20: review the Angular 20 breaking-changes list (deprecated symbols \
                 removed, zoneless change detection promotion).",
            ));
            rules.push(MigrationRule::note("Node 20.19+/22.12+ required."));
        }
        20 => {
            rules.push(MigrationRule::note(
                "Angular 21: consult the official update guide at https://update.angular.io. \
                 The catalog entry for v21 is best-effort (TypeScript/Node requirements not yet \
                 fully verified by this tool).",
            ));
        }
        21 => {
            rules.push(MigrationRule::note(
                "Angular 22: consult the official update guide at https://update.angular.io. \
                 The catalog entry for v22 is best-effort.",
            ));
        }
        _ => {
            rules.push(MigrationRule::note(format!(
                "Transition {from} -> {}: no curated rules; consult \
                 https://update.angular.io.",
                from + 1
            )));
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_transition_has_at_least_a_note() {
        for from in 6..=21 {
            let rules = step_rules(from, PlanOptions::default());
            assert!(!rules.is_empty(), "transition {from} should have rules");
        }
    }

    #[test]
    fn control_flow_rule_is_opt_in() {
        let without = step_rules(16, PlanOptions::default());
        assert!(without
            .iter()
            .all(|r| !matches!(r, MigrationRule::ControlFlowMigration)));
        let with = step_rules(
            16,
            PlanOptions {
                apply_control_flow: true,
            },
        );
        assert!(with
            .iter()
            .any(|r| matches!(r, MigrationRule::ControlFlowMigration)));
    }

    #[test]
    fn v13_removes_enable_ivy() {
        let rules = step_rules(12, PlanOptions::default());
        assert!(rules.iter().any(
            |r| matches!(r, MigrationRule::RemoveCompilerOption { key } if key == "enableIvy")
        ));
    }

    #[test]
    fn v14_removes_entry_components() {
        let rules = step_rules(13, PlanOptions::default());
        assert!(rules.iter().any(
            |r| matches!(r, MigrationRule::RemoveNgModuleField { field } if field == "entryComponents")
        ));
    }

    #[test]
    fn v17_removes_e2e() {
        let rules = step_rules(16, PlanOptions::default());
        assert!(rules
            .iter()
            .any(|r| matches!(r, MigrationRule::RemoveScript { script } if script == "e2e")));
        assert!(rules.iter().any(
            |r| matches!(r, MigrationRule::RemoveWorkspaceTarget { target } if target == "e2e")
        ));
        assert!(rules
            .iter()
            .any(|r| matches!(r, MigrationRule::DepRemove { package } if package == "protractor")));
    }
}
