# Rust 教程：用 5000 行代码把 Angular 项目从 v6 迁移到 v22

> 本文以仓库中的 `angular-migrator` 项目（`src/` 下约 30 个模块、39 个单元测试 +
> 6 个集成测试、全部通过 `cargo clippy` 且 0 warning）为真实示例，从零讲解 Rust
> 的工程实践。所有代码片段都直接取自本仓库，你可以在 `src/` 中对应文件里找到原文。

## 目录

1. [工程骨架：Cargo.toml 与 lib/bin 结构](#1-工程骨架cargotoml-与-libbin-结构)
2. [用结构体建模领域对象](#2-用结构体建模领域对象)
3. [用枚举表达"规则"这个抽象](#3-用枚举表达规则这个抽象)
4. [错误处理：anyhow 三板斧](#4-错误处理anyhow-三板斧)
5. [泛型与 trait：Map 集合](#5-泛型与-traitbtreemap-集合)
6. [Option 与模式匹配：处理"可能没有值"](#6-option-与模式匹配处理可能没有值)
7. [字节级解析：手写一个 HTML 扫描器](#7-字节级解析手写一个-html-扫描器)
8. [正则表达式与文本重写](#8-正则表达式与文本重写)
9. [把"数据"当代码：版本目录表](#9-把数据当代码版本目录表)
10. [模块化：从 main.rs 到 lib.rs](#10-模块化从-mainrs-到-librs)
11. [测试：单元测试 + 集成测试双保险](#11-测试单元测试--集成测试双保险)
12. [Cargo feature：可选依赖与离线模式](#12-cargo-feature可选依赖与离线模式)
13. [实战演练：完整跑通一次迁移](#13-实战演练完整跑通一次迁移)
14. [总结：从本例中带走的 Rust 心法](#14-总结从本例中带走的-rust-心法)

---

## 1. 工程骨架：Cargo.toml 与 lib/bin 结构

### 1.1 一个真实的 Cargo.toml

先看 `Cargo.toml`（本仓库根目录）：

```toml
[package]
name = "angular-migrator"
version = "0.1.0"
edition = "2021"
description = "Offline-first Angular project migration tool"
license = "MIT"

[features]
default = ["network"]
network = ["dep:ureq"]

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
regex = "1"
semver = "1"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
glob = "0.3"
walkdir = "2"
ureq = { version = "2", features = ["json"], optional = true }

[dev-dependencies]
tempfile = "3"

[[bin]]
name = "angular-migrator"
path = "src/main.rs"

[profile.release]
lto = true
```

几个值得注意的点：

- **`edition = "2021"`**：Rust 的"版本"叫 edition，2021 是最新版，影响语法特性。
- **`[[bin]]`**：显式声明二进制入口。一个 crate 可以同时是库（lib）和可执行文件（bin），
  这是"一个项目既能当 CLI 用、又能被其他程序 import"的关键。
- **`serde_json` 开了 `preserve_order`**：默认 `serde_json::Map` 是 `BTreeMap`（按键排序）。
  打开这个 feature 后改用 `IndexMap`，JSON 对象保持源文件中的键顺序——对"重写 package.json
  时不要打乱字段顺序"这种需求至关重要。
- **`ureq` 是 `optional = true`**：配合 `[features]` 里的 `network`，实现"不联网也能编译/运行"。

### 1.2 main.rs：极简入口

`src/main.rs` 总共 9 行：

```rust
use clap::Parser;

fn main() {
    let cli = angular_migrator::cli::Cli::parse();
    if let Err(err) = angular_migrator::cli::run(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
```

- `angular_migrator::cli::Cli` 暴露了库的模块路径，这说明本 crate 是 **lib + bin** 结构：
  所有逻辑都在 `src/lib.rs` 声明的库模块里，`main.rs` 只负责调用和退出。
- `{err:#}` 是 **alternate format**：打印 `anyhow` 错误时会展开整个错误链（`cause` 链），
  调试时非常有用。

### 1.3 lib.rs：模块声明即文档

`src/lib.rs`：

```rust
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
```

Rust 里 `//!` 是 **模块级文档注释**，会渲染进生成的 API 文档（`cargo doc`）。
`[`detect`]` 这样的写法是 **intra-doc link**，文档里会变成可点击的链接。

> **练习**：把 `src/main.rs` 的 `main` 函数去掉参数校验逻辑，改成用 `let cli: Cli = Cli::parse();`，
> 看看编译器如何帮你推断类型。Rust 的类型推断在这里非常激进。

---

## 2. 用结构体建模领域对象

`src/model.rs` 定义了整个工具的核心数据类型。看 `PackageJson`：

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct PackageJson {
    /// Original raw text (kept so untouched files are never rewritten).
    pub raw: String,
    /// Preserve-order parsed JSON.
    pub data: Value,
    /// Absolute path to the file.
    pub path: PathBuf,
}
```

### 2.1 `#[derive(Debug, Clone)]` 是什么

- **`Debug`**：让类型可以用 `{:?}` 打印，调试必备。
- **`Clone`**：让类型可以被显式复制（`.clone()`）。Rust 的赋值默认是 **move**（移动），
  没有隐式拷贝；需要"复制一份"时必须显式调 `.clone()`。

Rust 的 derive（派生）机制会**自动生成** trait 的标准实现。你能 derive 的常用 trait：

| Trait | 作用 | 本仓库用到的地方 |
|---|---|---|
| `Debug` | `{:?}` 格式化 | 几乎所有结构体/枚举 |
| `Clone` | 显式深拷贝 | `PackageJson`, `Project` |
| `PartialEq, Eq` | `==` / `!=` | `MigrationRule` 测试断言 |
| `Default` | 提供 `::default()` | `ControlFlowResult`, `MigrateOptions` |
| `Copy` | 隐式拷贝（按位复制） | `FileKind`, `PlanOptions` |

注意区分 `Clone` 与 `Copy`：`Copy` 是隐式的、廉价的（如整数、`bool`、小枚举），
`Clone` 是显式的、可能昂贵的（如 `String`、`Vec`）。`FileKind` 是 C 风格小枚举，`#[derive(Clone, Copy)]` 双份。

### 2.2 `impl` 块与"胖"结构体

Rust 把方法和数据放在一起（`impl PackageJson { ... }`），方法用 `&self` / `&mut self` 区分只读和可变：

```rust
impl PackageJson {
    pub fn name(&self) -> &str {
        self.data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed project)")
    }

    /// All dependency maps (dependencies + devDependencies), merged.
    pub fn all_dependencies(&self) -> BTreeMap<String, String> {
        let mut all = BTreeMap::new();
        if let Some(d) = self.data.get("dependencies").and_then(|v| v.as_object()) {
            for (k, v) in d {
                if let Some(s) = v.as_str() {
                    all.insert(k.clone(), s.to_string());
                }
            }
        }
        // ...devDependencies 同理
        all
    }
}
```

- `&self` 不可变借用，`&mut self` 可变借用。编译器强制这个区分，从根上杜绝"不小心改坏了数据"。
- `&str` 是**借用**的字符串视图（不拥有数据），`String` 是**拥有**的字符串。
  返回 `&str` 意味着"我只让你看，不给你拷贝"。这里返回的是 `self.data` 内部字段的引用，
  生命周期由借用检查器自动保证。

### 2.3 为什么没有"构造函数"？

Rust 没有构造函数语法，惯例是用**关联函数**（`Self::new()`）或直接构造：

```rust
Ok(PackageJson {
    raw,
    data,
    path: path.to_path_buf(),
})
```

`raw` 和 `data` 都是移动进来的（函数参数直接给字段），`path` 需要 `to_path_buf()` 复制一份。
这叫 **struct literal**，字段名与变量名相同时可以简写为 `raw,` 而不是 `raw: raw,`。

---

## 3. 用枚举表达"规则"这个抽象

这是本项目设计上最核心的地方。`MigrationRule` 用**一个枚举**表达了"迁移动作"的所有可能性：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationRule {
    /// Update an existing dependency to a version.
    DepUpdate { package: String, version: String },
    /// Remove a dependency entirely.
    DepRemove { package: String },
    /// Add a dependency (defaults to devDependencies).
    DepAdd { package: String, version: String, dev: bool },
    /// Regex replacement over files matching a glob.
    Replace { glob: String, pattern: String, replacement: String, kind: FileKind },
    /// Remove a top-level metadata field from every `@NgModule({ ... })` literal.
    RemoveNgModuleField { field: String },
    /// Remove a key from `angularCompilerOptions` in all tsconfig files.
    RemoveCompilerOption { key: String },
    /// Set a key/value inside `angularCompilerOptions`.
    SetCompilerOption { key: String, value: String },
    /// Strip deprecated `ng` CLI flags from npm scripts.
    StripScriptFlags { flags: Vec<String> },
    /// Remove a script (e.g. the `e2e` script in Angular 17).
    RemoveScript { script: String },
    /// Remove an architect target (e.g. `e2e`) from every project in angular.json.
    RemoveWorkspaceTarget { target: String },
    /// Apply the structural-directive -> control-flow rewrite (*ngIf/*ngFor).
    ControlFlowMigration,
    /// Informational only: a manual step that cannot be automated safely.
    Note { text: String },
}
```

### 3.1 数据型枚举（struct-like variants）

Rust 枚举的变体可以携带任意数据，语法类似结构体：

- `DepUpdate { package, version }` —— 命名字段，语义清晰；
- `ControlFlowMigration` —— 无字段，就是一个标记；
- `Note { text }` —— 特殊变体，表示"只提示、不执行"。

### 3.2 "模式匹配代替继承"

在其他语言里，这个需求可能用"抽象基类 + 多态"实现。Rust 的做法是枚举 + `match`。
关键好处：**match 是穷尽的**——编译器要求你处理所有变体，新增一个变体后，
所有 `match` 它的地方编译报错，逼你逐个检查。这叫"编译期穷尽性检查"，是 Rust 表达
"密封抽象"的核心能力。

看 `src/plan.rs` 里的 `describe_rule`，把"规则"渲染成人类可读文本：

```rust
pub fn describe_rule(rule: &crate::model::MigrationRule) -> String {
    use crate::model::MigrationRule as R;
    match rule {
        R::DepUpdate { package, version } => format!("update {package} to {version}"),
        R::DepRemove { package } => format!("remove dependency {package}"),
        R::Replace { glob, pattern, .. } => format!("regex replace `{pattern}` in {glob}"),
        R::RemoveNgModuleField { field } => format!("remove `{field}` from every @NgModule metadata"),
        R::Note { .. } => unreachable!("notes are rendered separately"),
        // ... 其余变体
    }
}
```

- `{package}` 是 **捕获绑定**：从变体里"解包"出字段。
- `..` 是"其余字段忽略"（这里 `Replace` 的 `replacement`、`kind` 用不到）。
- `R::Note { .. } => unreachable!(...)`：当已知调用方保证不会传入该变体时，用 `unreachable!`
  表达"这里逻辑上到不了"。

### 3.3 另一个枚举：把"行为"装进枚举

`src/control_flow.rs` 里的 `Decision` 枚举展示了**函数内部的分支决策**：

```rust
enum Decision {
    MigrateIf(String),
    MigrateFor(ForExpr),
    Keep,
    Skip(&'static str),
}
```

一个 `decide()` 函数把所有判断逻辑集中成"对每个元素，要么迁移、要么跳过、要么原样保留"，
调用方再 `match` 处理。枚举让**判断**和**执行**分离，代码结构非常清晰。

> **练习**：给 `MigrationRule` 增加一个变体 `RenameScript { from: String, to: String }`。
> 然后 `cargo build`，观察编译器列出所有需要补 `match` 分支的地方。这就是"穷尽匹配"的威力。

---

## 4. 错误处理：anyhow 三板斧

Rust 没有异常（exception）。错误用 `Result<T, E>` 返回值显式传递。本仓库统一用
`anyhow` 这个库来简化错误处理。

### 4.1 签名：函数返回 `Result<...>`

```rust
use anyhow::{Context, Result};

pub fn detect(root: &Path) -> Result<Project> {
    if !root.is_dir() {
        anyhow::bail!("path is not a directory: {}", root.display());
    }
    // ...
}
```

- `Result<Project>` 是 `Result<Project, anyhow::Error>` 的简写——具体错误类型是啥不关心，
  反正都能链式传播。
- `bail!` 宏：**立即返回一个错误**，相当于其他语言的 `throw`。

### 4.2 `?` 运算符：错误自动向上传播

```rust
let raw = std::fs::read_to_string(&p)
    .with_context(|| format!("failed to read {}", p.display()))?;
```

- `?` 是 Rust 最常用的错误处理语法：`Err` 就提前返回，`Ok` 就把值解包出来。
  不用写一堆 `if (err) return err;`。
- `.with_context(...)` 给错误**添加上下文**。`Context` trait 来自 anyhow，
  让底层错误（如 `IO error`）带上"是读哪个文件失败"的信息。

### 4.3 调用方怎么消费

`src/main.rs`：

```rust
if let Err(err) = angular_migrator::cli::run(cli) {
    eprintln!("error: {err:#}");
    std::process::exit(1);
}
```

`{err:#}` 会打印完整的错误链：

```
error: no package.json found at /tmp/foo (is this an Angular project?)
```

`detect.rs` 里还用到了 `.ok_or_else(...)`，把 `Option` 转成 `Result`：

```rust
let from = project
    .angular_major()
    .ok_or_else(|| anyhow::anyhow!("no `@angular/core` dependency found in package.json"))?;
```

> 什么时候用 `unwrap()`？只在测试里或逻辑上不可能失败的地方。生产代码一律 `?` / `bail!` /
> `with_context`。

---

## 5. 泛型与 trait：BTreeMap 集合

`model.rs` 大量使用 `BTreeMap<String, String>`。Rust 标准库的集合有两个：
`HashMap`（无序、快）和 `BTreeMap`（按键有序）。本仓库用 `BTreeMap`，因为
**输出要有确定性**（脚本规则按字母序处理，测试也好断言）。

泛型体现在 `BTreeMap<K, V>` 的两个类型参数上。集合上的常用操作：

```rust
pub fn scripts(&self) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(s) = self.data.get("scripts").and_then(|v| v.as_object()) {
        for (k, v) in s {
            if let Some(str) = v.as_str() {
                out.insert(k.clone(), str.to_string());
            }
        }
    }
    out
}
```

`if let` 是"如果模式匹配就执行"的语法糖——这里是 `Option` 解包的常用写法。
`self.data` 是 `serde_json::Value`，`get("scripts")` 返回 `Option<&Value>`，
`.and_then(...)` 在值为 `Some` 时继续转换，全程不 panic。

`BTreeMap` 的键是 `String`，`k.clone()` 是因为 `k` 是 `&String`（借用的），
要存进新 Map 必须拥有它。**借用（&）与拥有（owned）的区分**贯穿整个 Rust 编程。

> **练习**：把 `all_dependencies()` 改成返回 `HashMap<String, String>`，跑一遍测试，
> 看看哪里因为顺序依赖而失败。体会"可预测的顺序"在工程里的价值。

---

## 6. Option 与模式匹配：处理"可能没有值"

Angular 项目的检测必须处理各种"字段可能不存在"的情况。`model.rs` 里的 `Project`：

```rust
impl Project {
    /// The `@angular/core` major version, if the project uses Angular.
    pub fn angular_major(&self) -> Option<u32> {
        self.package
            .dependency("@angular/core")
            .as_deref()
            .and_then(parse_major)
    }

    /// Whether `angular.json` declares an `e2e` target on any project.
    pub fn has_e2e_target(&self) -> bool {
        let Some(w) = &self.workspace else {
            return false;
        };
        let Some(projects) = w.get("projects").and_then(|v| v.as_object()) else {
            return false;
        };
        projects.values().any(|p| {
            p.get("architect")
                .and_then(|a| a.as_object())
                .map(|a| a.contains_key("e2e"))
                .unwrap_or(false)
        })
    }
}
```

这一段展示了 **let-else**（Rust 1.65 引入的 2021 edition 语法）：

```rust
let Some(w) = &self.workspace else {
    return false;
};
```

"如果 `self.workspace` 是 `None` 就提前返回，否则把值绑定为 `w`"。
相比 `match`，`let-else` 让"提前返回 + 解包"一行完成，可读性极佳。

再看 `parse_major`——用**纯函数式**的方式解析 npm 版本号：

```rust
pub fn parse_major(spec: &str) -> Option<u32> {
    let mut t = spec.trim();
    if t.is_empty() || t == "*" || t == "latest" || t == "next" {
        return None;
    }
    if t.contains("git+") || t.starts_with("file:") || t.starts_with("workspace:") {
        return None;
    }
    if let Some(rest) = t.strip_prefix("npm:") {
        t = rest;
    }
    t = t.trim_start_matches(['^', '~', '>', '<', '=', 'v', ' ', '\t']);
    // npm range like ">=12.0.0 <13.0.0"
    if let Some(first) = t.split_whitespace().next() {
        t = first;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}
```

- 全程返回 `Option<u32>`：无法解析就 `None`，调用方决定怎么处理（报错 or 忽略）。
- `strip_prefix`、`take_while`、`parse().ok()` 都是 **Option 友好**的标准库方法。
- 字符串的 `char` 与 `byte`：`is_ascii_digit()` 判断单个字符；注意中文、emoji 是
  多字节 UTF-8，用 `chars()` 迭代保证安全。这正是 `digits` 用 `chars()` 的原因。

---

## 7. 字节级解析：手写一个 HTML 扫描器

`src/control_flow.rs` 是"不引入重型解析库、手写一个够用的解析器"的典型例子。
核心思路：**把模板字符串当作字节数组处理，跳过字符串/注释，按规则扫描**。

### 7.1 逐字节扫描开标签

```rust
fn parse_open_tag(src: &str, lt: usize, end: usize) -> Option<(Element, usize)> {
    let b = src.as_bytes();
    let n = b.len().min(end);
    let mut i = lt + 1;
    // Comments / doctypes are not elements.
    if i < n && (b[i] == b'!' || b[i] == b'?') {
        return None;
    }
    let name_start = i;
    while i < n && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b':' | b'_')) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = src[name_start..i].to_string();
    // ... 继续解析属性
}
```

- `src.as_bytes()` 把字符串变成 `&[u8]`，之后全是 **索引运算**（`b[i]`），没有迭代器开销。
- `matches!(b[i], b'-' | b':' | b'_')` 是简洁的字节模式匹配。
- 注意边界检查：`i < n` 遍布每个循环——手写扫描器最怕越界，Rust 会在 debug 模式 panic、
  release 模式可能越界，所以宁可多写条件。

### 7.2 用"同名计数器"找配对闭合标签

HTML 允许同名元素嵌套（`<div><div></div></div>`）。`find_close_tag` 用**深度计数**解决：

```rust
fn find_close_tag(src: &str, name: &str, from: usize, end: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut depth = 1usize; // the element itself
    let mut i = from;
    while i < n {
        if b[i] != b'<' { i += 1; continue; }
        if i + 1 < n && b[i + 1] == b'/' {
            // closing tag
            let mut j = i + 2;
            while j < n && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b':' | b'_')) {
                j += 1;
            }
            let cname = &src[i + 2..j];
            if cname == name {
                depth -= 1;
                if depth == 0 {
                    // ... 返回闭合标签结束位置
                }
            }
            i = j;
            continue;
        }
        // open tag inside: 跳过到 '>'（注意引号）
        // ... depth += 1 when oname == name
    }
    None
}
```

扫描器需要**跳过引号内的内容**（`<div title=">">` 里的 `>` 不是标签结束）。代码里专门
维护了一个 `in_q` 状态机：

```rust
let mut in_q: Option<u8> = None;
while k < n {
    let c = b[k];
    if let Some(q) = in_q {
        if c == b'\\' { k += 2; continue; }   // 转义字符
        if c == q { in_q = None; }            // 引号结束
        k += 1; continue;
    }
    if c == b'"' || c == b'\'' { in_q = Some(c); }   // 进入引号
    else if c == b'>' { break; }                     // 标签真正结束
    k += 1;
}
```

### 7.3 递归下降重写

`process_range` 是递归函数：遇到可迁移元素就重写，遇到保留元素就**递归进入其内容**，
保证嵌套指令也能处理：

```rust
Decision::MigrateIf(expr) => {
    stats.migrated += 1;
    let indent = leading_ws_of_line(src, lt);
    let open = rebuild_open_tag(&el, &["*ngIf"]);
    let mut block = open;
    if !el.self_closing {
        let inner = process_range(src, el.content_range.clone(), stats);
        block.push_str(&inner);
        block.push_str(&close);
    }
    out.push_str(&format!("{indent}@if ({expr}) {{\n"));
    out.push_str(&reindent(&block, &format!("{indent}  ")));
    out.push('\n');
    out.push_str(&indent);
    out.push_str("}\n");
    cursor = el.close_end.max(el.open_range.end);
}
```

- 输出用 `String` 拼接，`format!` 宏做模板字符串。
- 空格缩进用 `leading_ws_of_line` 提取原行缩进、`reindent` 给每行加两级缩进，
  保证生成的 `@if` 块缩进正确。

> **设计要点**：这个解析器刻意**保守**——遇到 `*ngIf="cond; else other"` 这种
> 微语法（micro-syntax）就 `Skip` 并输出 warning，绝不猜测。测试里专门有一条
> `skips_ng_template_else` 验证"看不懂就跳过"。**工具的正确性 > 覆盖率**，这是
> 代码改写工具最重要的工程原则。

---

## 8. 正则表达式与文本重写

`src/transforms.rs` 提供了两层文本改写机制：**全局正则替换**和**结构感知编辑**。

### 8.1 正则替换

```rust
pub fn apply_regex_replace(
    root: &Path,
    glob_pattern: &str,
    pattern: &str,
    replacement: &str,
    kind: FileKind,
    dry: bool,
) -> Result<Vec<std::path::PathBuf>> {
    let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex `{pattern}`: {e}"))?;
    let mut changed = Vec::new();
    for path in files_for_glob(root, glob_pattern, kind) {
        let raw = std::fs::read_to_string(&path)?;
        let next = re.replace_all(&raw, replacement).into_owned();
        if next != raw {
            if !dry {
                std::fs::write(&path, next)?;
            }
            changed.push(path);
        }
    }
    Ok(changed)
}
```

- `regex::Regex::new` 返回 `Result`——**非法正则在运行时报错**，不 panic。
- `replace_all(...).into_owned()`：`re` 的替换返回 `Cow<str>`（借用或拥有二选一），
  需要变成 `String` 时用 `into_owned()`。这是 `Cow`（Copy-on-Write）类型的典型用法。
- **只写有变化时**（`next != raw`）：避免无谓的磁盘写入和 mtime 变化。
- `dry` 参数贯穿整个项目：**dry-run 模式下不落盘**，这是迁移工具的标配。

### 8.2 结构感知编辑：跳过字符串和注释

`@NgModule({ entryComponents: [...] })` 的删除不能简单 `replace`，因为：

- 字符串里可能恰好包含 `entryComponents` 字样；
- 嵌套对象里可能有同名属性；
- 键与冒号之间可能有注释。

`transforms.rs` 实现了一组"跳过"工具函数：

```rust
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    let quote = bytes[i];
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' { i += 2; continue; }   // 转义
        if bytes[i] == quote { return i + 1; }
        i += 1;
    }
    i
}

fn skip_ws_comments(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() { i += 1; }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i = skip_line_comment(bytes, i);
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i = skip_block_comment(bytes, i);
        } else {
            return i;
        }
    }
}
```

主扫描函数 `find_ng_module_object_ranges` 遍历整个源码，遇到字符串/注释就跳过，
遇到 `NgModule` 标识符就检查后面是否是 `(` `{`，再用 `find_matching`（括号深度计数）
定位整个对象字面量范围。

```rust
b'\'' | b'"' | b'`' => { i = skip_string(bytes, i); }
b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => { i = skip_line_comment(bytes, i); }
_ if is_ident_start(c) => {
    let start = i;
    while i < bytes.len() && is_ident_part(bytes[i]) { i += 1; }
    let name = &src[start..i];
    if name == "NgModule" {
        let j = skip_ws_comments(bytes, i);
        if j < bytes.len() && bytes[j] == b'(' {
            // 找到调用括号内匹配的 ')'
        }
    }
}
```

> **模式**：手写解析器的通用套路是"主循环 + 状态机"。主循环推进字节，遇到特殊状态
> （字符串、注释、括号）就跳到对应处理函数，处理函数返回"下次从哪继续"。

---

## 9. 把"数据"当代码：版本目录表

`src/catalog.rs` 把"每个 Angular 大版本对应的包版本"组织成数据，而不是散落各处的
if-else。这就是**数据驱动编程**。

核心结构（节选）：

```rust
pub struct AngularMajor {
    /// Confirmed against the npm registry.
    pub confirmed: bool,
    pub core: &'static str,
    pub cli: &'static str,
    pub material: &'static str,
    pub zone_js: &'static str,
    pub rxjs: &'static str,
    pub tslib: &'static str,
    pub typescript: &'static str,
    pub typescript_max: &'static str,
    pub node: &'static str,
}

pub static CATALOG: &[(&str, AngularMajor)] = &[
    ("6", AngularMajor {
        confirmed: true,
        core: "6.1.10",
        cli: "6.2.9",
        material: "6.4.7",
        zone_js: "0.8.28",
        rxjs: "6.6.7",
        typescript: "2.7",
        typescript_max: "2.9",
        node: ">= 8.9",
    }),
    // ... 7 到 22
];
```

`&'static str` 是**静态生命周期**的字符串切片——在编译期就确定的字面量，随程序
整个生命周期存活，不分配堆内存。用 `static` 表 + `&'static str` 表达"不可变的程序内
数据表"是 Rust 的惯用做法。

配套的查询函数：

```rust
pub fn catalog_major(major: u32) -> Option<&'static AngularMajor> {
    CATALOG
        .iter()
        .find(|(m, _)| m.parse::<u32>().ok() == Some(major))
        .map(|(_, entry)| entry)
}
```

`find` 返回 `Option<&item>`，`map` 取出 `AngularMajor` 引用。`iter().find().map()`
组合是 Rust 处理"从集合里查找"的标准姿势。

> 6-20 的版本号是**实际对照 npm registry 验证过**的，21-22 是估算（`confirmed: false`）。
> 数据表带 `confirmed` 标志，工具输出会提示用户哪些条目是 best-effort——**让数据的可信度
> 可见**，也是工程素养的体现。

---

## 10. 模块化：从 main.rs 到 lib.rs

Rust 的模块系统三件套：**crate（包）→ module（模块）→ item（项目）**。

### 10.1 模块树

```
src/
├── lib.rs          # 库根：声明所有 pub mod
├── main.rs         # 二进制根：只有 9 行
├── model.rs        # 核心数据类型
├── detect.rs       # 项目检测
├── plan.rs         # 迁移规划
├── rules.rs        # 每大版本的规则（数据驱动）
├── migrate.rs      # 编排器：应用整个计划
├── transforms.rs   # 文本改写引擎
├── tsconfig.rs     # tsconfig 编辑
├── control_flow.rs # 控制流重写
├── dependencies.rs # package.json 操作
├── catalog.rs      # 版本目录
├── thirdparty.rs   # 第三方依赖建议
├── npm.rs          # npm registry 查询
├── report.rs       # Markdown 报告
└── cli.rs          # clap CLI
```

### 10.2 可见性

- `pub mod` / `pub fn` / `pub struct`：**对 crate 外部可见**。
- 默认私有：只有**当前模块及其子模块**可见。`main.rs` 能调用 `angular_migrator::cli::run`
  是因为 `lib.rs` 里 `pub mod cli;` 且 `cli::run` 是 `pub fn`。
- 本仓库里 `strip_flags`、`remove_top_level_property` 等内部辅助函数刻意**不 pub**，
  封装实现细节，只暴露稳定接口。

### 10.3 依赖方向

```
cli.rs ──> migrate.rs ──> plan.rs ──> rules.rs / catalog.rs / thirdparty.rs
   │              │
   └────> detect.rs ──> model.rs ──┐
                                  ├─> transforms.rs / dependencies.rs / tsconfig.rs
                                  └─> control_flow.rs
```

`model.rs` 是**底层的共享数据层**，其他模块都依赖它；`cli.rs` 是顶层，依赖所有执行模块。
这种"数据在底层、行为在中层、入口在顶层"的分层，让依赖单向流动、可测试性高。

---

## 11. 测试：单元测试 + 集成测试双保险

### 11.1 单元测试：内嵌 `#[cfg(test)]`

Rust 的单元测试**直接写在源码文件底部**的 `mod tests` 里：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_specifiers() {
        assert_eq!(parse_major("^12.2.0"), Some(12));
        assert_eq!(parse_major("~13.1.5"), Some(13));
        assert_eq!(parse_major("14"), Some(14));
        assert_eq!(parse_major("*"), None);
        assert_eq!(parse_major("git+https://github.com/foo/bar.git"), None);
    }
}
```

- `#[cfg(test)]`：**只在测试构建时**编译这个模块，发布时不带。
- `use super::*;`：引入父模块的所有项，测试直接访问私有函数——**单元测试能测私有 API**，
  这是 Rust 测试的杀手锏。
- `assert_eq!` / `assert!` / `assert_ne!` 是断言宏；失败会打印左右值。

`model.rs` 里测试了**边界情况**（空、通配符、git 地址、workspace 别名）：

```rust
assert_eq!(parse_major("*"), None);
assert_eq!(parse_major("latest"), None);
assert_eq!(parse_major("workspace:*"), None);
assert_eq!(parse_major("npm:foo@^1.0.0"), None);
```

`transforms.rs` 测试了"字符串里的假 `@NgModule` 不动"：

```rust
#[test]
fn leaves_strings_untouched() {
    let src = r#"
const s = "@NgModule({ entryComponents: 'fake' })";
@NgModule({
  declarations: [AppComponent]
})
export class AppModule {}
"#;
    let (out, count) = remove_ng_module_field(src, "entryComponents");
    assert_eq!(count, 0);
    assert!(out.contains("const s"));
}
```

`r#"..."#` 是**原始字符串字面量**，可以包含 `"` 而不需要转义，非常适合放带引号的测试数据。

### 11.2 测试辅助：临时目录

`dependencies.rs` 的测试用一个 `AtomicU64` 计数器保证临时文件不冲突：

```rust
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn pkg_from(content: &str) -> PackageJson {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("am-test-pkg-{}-{id}.json", std::process::id()));
    std::fs::write(&path, content).unwrap();
    // ...
}
```

`AtomicU64` 是**原子类型**，多线程并发执行测试时也能安全递增，保证文件名唯一。

### 11.3 集成测试：真实文件系统

`tests/integration.rs` 在**另一个 crate** 里，只能调用 crate 的**公开 API**。
它把 fixture 复制到临时目录，跑完整迁移再断言结果：

```rust
let pkg = json!({
    "dependencies": { "@angular/core": "^17.3.12", ... },
    "scripts": { "build": "ng build", ... },
});
assert_eq!(pkg["scripts"]["build"], "ng build");
```

`tempfile::TempDir` 是集成测试标配——测试结束自动删除目录，不污染工作区。

### 11.4 跑测试

```bash
cargo test              # 全部测试
cargo test model        # 过滤：名字含 "model" 的测试
cargo test -- --nocapture   # 显示 println! 输出
cargo test parses_common_specifiers  # 单个测试
```

> 规则 `every_transition_has_at_least_a_note` 遍历 6..=21 所有迁移步，断言每步至少有一条
> 规则。这种"**数据表完备性**"测试，能防止日后往 `rules.rs` 加新大版本时漏掉规则。

---

## 12. Cargo feature：可选依赖与离线模式

本项目强调 **offline-first**（离线优先）：核心功能不联网。npm registry 查询被
藏在可选依赖后面。

### 12.1 声明

```toml
[features]
default = ["network"]
network = ["dep:ureq"]

[dependencies]
ureq = { version = "2", features = ["json"], optional = true }
```

- `ureq` 标记 `optional = true`：**不编译进默认构建**。
- `network` feature 打开 `ureq` 依赖（`dep:ureq` 语法是让 feature 控制依赖启用）。
- `default = ["network"]`：默认开启网络能力，但用户可以 `--no-default-features` 关掉。

### 12.2 在代码里用 `cfg!(feature = "network")`

`src/thirdparty.rs` 里区分离线/联网：

```rust
pub fn suggest_with_registry(project: &Project, to: u32, offline: bool) -> Vec<DependencySuggestion> {
    let local = suggest(project, to);
    if offline || !cfg!(feature = "network") {
        return local;
    }
    // 编译了 network feature 且用户未 --offline，才走 npm registry 查询
    // ...
    local
}
```

- `cfg!(...)` 是**编译期求值、返回 `bool` 的宏**：feature 没启用时，整个 `if` 条件恒为
  `true`，分支代码在编译期被标记为不可达，相关依赖在**链接期**不会引入（可选依赖
  `ureq` 没被启用就不编译）。用 `cfg!` 而不是 `#[cfg]` 属性，让"是否联网"成为一个
  普通的运行时分支，逻辑更清晰。
- `suggest_with_registry` 保留 `offline` 参数：即使编译了网络支持，用户显式
  `--offline` 时也跳过联网——**能力与策略分离**。
- 另一处可看到特性门控：`Cargo.toml` 里 `ureq` 的 `optional = true` 与 `network`
  feature 的 `dep:ureq` 声明配对，feature 关闭时这个依赖根本不会进入构建图。

### 12.3 验证

```bash
cargo build --no-default-features   # 不带网络依赖的构建
cargo build                         # 默认（带网络）
cargo build --features network      # 显式启用
```

> 这是 Rust 的 **Cargo feature 系统**，常用于"可选功能、可选平台、可选依赖"。
> 配合 `#[cfg]`，可以在**编译期**裁剪功能，不产生任何运行时开销。

---

## 13. 实战演练：完整跑通一次迁移

结合前面的所有概念，我们端到端跑一次：

```bash
# 1. 构建发布版
cargo build --release

# 2. 分析项目（detect 模块）
./target/release/angular-migrator analyze fixtures/ng12-app

# 3. 只规划、不动文件（plan 模块，离线）
./target/release/angular-migrator plan fixtures/ng12-app --target 17 --offline

# 4. 真实迁移（migrate 模块，带控制流重写）
./target/release/angular-migrator migrate fixtures/ng12-app --target 17 \
    --apply-control-flow --offline
```

迁移日志节选：

```
Migrating Angular 12 -> 17 (13 -> 14 -> 15 -> 16 -> 17)
--- Angular 12 -> 13 ---
  - remove `enableIvy` from angularCompilerOptions
  - strip deprecated CLI flags from scripts: ["--prod", ...]
  - remove dependency @angular-devkit/build-ng-packagr
--- Angular 16 -> 17 ---
  - removed `e2e` target from angular.json projects
  - removed script `e2e`
  - removed dependency protractor
  ~ control flow: src/app/app.component.html
~ aligned 16 Angular/toolchain dependencies to v17
? third-party: @ngrx/store ^12.5.2 -> ^17.0.0
rewrote package.json

Warnings:
  ! skipped `div` at byte 128: ngIf uses micro-syntax (else/then/alias); migrate manually

Control-flow: 2 migrated, 1 skipped (see warnings).
```

验证结果：

```bash
grep -n '"@angular/core"' fixtures/ng12-app/package.json
#  "@angular/core": "^17.3.12"

grep -rn 'ngIf\|ngFor\|@if\|@for' fixtures/ng12-app/src/app/app.component.html
#  @if (loaded) {
#  @for (item of items; track item) {
```

每个模块在链路中的角色一览：

| 模块 | 职责 | 对应教程章节 |
|---|---|---|
| `model.rs` | 数据类型 | 2, 3, 6 |
| `detect.rs` | 读文件、探测版本 | 4 |
| `catalog.rs` | 版本数据表 | 9 |
| `rules.rs` | 每步的规则（枚举） | 3 |
| `plan.rs` | 拼接迁移路径 | 3, 6 |
| `dependencies.rs` | package.json 操作 | 2, 5 |
| `transforms.rs` | 源码改写 | 8 |
| `tsconfig.rs` | tsconfig 编辑 | 4, 8 |
| `control_flow.rs` | HTML 重写 | 7 |
| `thirdparty.rs` / `npm.rs` | 第三方建议 | 12 |
| `migrate.rs` | 编排一切 | 4 |
| `cli.rs` | 命令行入口 | 1 |

---

## 14. 总结：从本例中带走的 Rust 心法

1. **枚举 + match 穷尽匹配**是表达"封闭抽象"的首选（`MigrationRule`），编译器帮你
   检查所有分支。
2. **`Result` / `Option` 是显式的**：错误和"没有值"不可能被悄悄忽略；`?`、`bail!`、
   `with_context`、`let-else` 让错误处理既安全又简洁。
3. **所有权与借用**无处不在：`&str` vs `String`、`&Path` vs `PathBuf`、`clone()` 的时机。
   模型层用 `BTreeMap` 保证可预测的顺序。
4. **手写解析器的套路**：字节级扫描 + 跳过字符串/注释 + 括号深度计数。保守优于激进，
   看不懂就跳过并警告。
5. **数据驱动**：版本目录是数据（`&'static` 表 + `confirmed` 标志），规则是枚举数据，
   行为代码集中且短。
6. **feature 开关**实现编译期功能裁剪，配合 `--offline` 实现离线优先。
7. **测试分层**：`#[cfg(test)]` 单元测试测私有函数，`tests/` 集成测试走公开 API +
   真实临时目录，数据表用"完备性"测试兜底。
8. **工程素养内化到代码里**：dry-run 不落盘、只重写有变化的文件、给用户 warning 而非
   静默失败、cli 在出错时打印完整错误链。

## 延伸阅读

- [The Rust Book](https://doc.rust-lang.org/book/) — 官方入门书
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/) — 示例驱动学习
- `cargo doc --open` — 生成本项目文档，看 `//!` 注释如何渲染
- 试试扩展本工具：给 `MigrationRule` 加一个 `RenameScript` 变体，跑 `cargo build`，
  看编译器如何带你找到所有需要修改的地方
