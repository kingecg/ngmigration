//! angular-migrator: an offline-first Angular project migration tool.
//!
//! The library is organized around four phases:
//!   1. [`detect`]  - parse `package.json`, `angular.json`, `tsconfig*.json`
//!   2. [`plan`]    - build an ordered major-by-major migration plan
//!   3. [`migrate`] - apply the plan (source transforms, config edits, deps)
//!   4. [`report`]  - human-readable plan/migration reports

pub mod catalog;
pub mod cli;
pub mod control_flow;
pub mod dependencies;
pub mod detect;
pub mod migrate;
pub mod model;
pub mod npm;
pub mod plan;
pub mod report;
pub mod rules;
pub mod thirdparty;
pub mod transforms;
pub mod tsconfig;
