# Nix

> Additive to STANDARDS.md. Read that first. Everything here is Nix-specific.
>
> Covers: Nix language, flake conventions, NixOS module patterns, derivation packaging, environment selection, overlays, cross-compilation, musl static builds, lazy evaluation debugging, dependency auditing.
>
> **Key decisions:** nixfmt formatter, let-in over rec, explicit pkgs over with, Crane for Rust, .follows on transitive nixpkgs, no lookup paths, final/prev overlay convention, nativeBuildInputs/buildInputs split for cross, pkgsStatic for musl, --show-trace always, vulnix in CI.

---

## Language fundamentals

### Types

Nix has very few types. Simplicity enables reproducibility.

**Primitive types:**

| Type | Examples | Notes |
|------|----------|-------|
| String | `"hello"`, `''multi-line''` | Interpolation with `${}`. Concatenation with `+`. |
| Boolean | `true`, `false` | Only booleans work in `if`. `null` is NOT falsy. |
| Integer | `42`, `-1` | |
| Float | `3.14` | Coerces with integers automatically. |
| Null | `null` | Distinct from `false`. Signifies absence. |
| Path | `./foo.nix`, `/etc/nixos` | Built-in type, not a string. Important for flake purity. |

**Compound types:**

| Type | Syntax | Notes |
|------|--------|-------|
| Attribute set (attrset) | `{ key = value; }` | Semicolons required. The fundamental data structure. |
| List | `[ 1 "two" 3 ]` | Space-separated. Heterogeneous. Concatenation with `++`. |

### Functions

```nix
# Single argument, single return value. ALWAYS.
x: x + 1

# Application uses space (not parentheses)
(x: x + 1) 2    # => 3

# Multi-argument via currying
x: y: x + y

# Attrset destructuring (most common pattern)
{ foo, bar }: foo + bar

# With default values
{ foo, bar ? "default" }: foo + bar

# With catch-all for extra args
{ foo, bar, ... }: foo + bar
```

### Key expressions

**`let ... in`**: Local bindings. The workhorse of factoring out code.

**`if ... then ... else`**: Everything is an expression. `if` returns a value.

**`inherit`**: Shorthand for `x = x` in attrsets. NOT OOP inheritance.

**`with`**: Brings attrset keys into scope. Use sparingly (see anti-patterns).

**`//`**: Shallow merge of attrsets. Right takes precedence. WARNING: nested attrsets are replaced entirely.

---

## Style and formatting

### Formatter

**nixfmt** (RFC 166): the official Nix formatter. Not alejandra.

```bash
nixfmt file.nix          # Format
nixfmt --check file.nix  # Check without modifying
```

### Naming conventions

| Context | Convention | Example |
|---------|-----------|---------|
| Files | `kebab-case.nix` | `desktop-gnome.nix`, `service-config.nix` |
| Attribute names | `camelCase` | `buildInputs`, `shellHook`, `defaultPackage` |
| NixOS options | `dot.separated.camelCase` | `services.myapp.enable` |
| Variables | `camelCase` | `craneLib`, `rustToolchain` |
| Flake outputs | Follow schema exactly | `packages`, `nixosConfigurations`, `devShells` |

### Indentation

Two spaces. No tabs.

### Comments

```nix
# Single-line comment

/* Multi-line comment
   spanning multiple lines */
```

Comments explain **why**, not what. Same philosophy as all other standards.

### String style

- Short strings: double quotes `"hello world"`
- Multi-line: double single quotes `''...''` (trims leading whitespace)
- Always quote URLs (RFC 45). No bare URL syntax.

---

## Flake structure

Every flake has three top-level attributes: `description`, `inputs`, `outputs`.

### Input conventions

- Pin nixpkgs to a specific branch
- **Always use `.follows`** for transitive nixpkgs dependencies
- Without `.follows`, different inputs pull different nixpkgs versions, breaking reproducibility

### Output schema

System-specific outputs go under `packages.<system>`, `devShells.<system>`, `checks.<system>`. System-independent outputs (`nixosConfigurations`, `nixosModules`, `overlays`) go at the top level.

### Multi-system pattern

Use `lib.genAttrs` or `flake-utils.lib.eachDefaultSystem` to avoid repetition per architecture. Never put system-independent outputs inside `eachDefaultSystem`.

### Lock file

`flake.lock` is auto-generated and pins exact versions. **Commit it.** It IS reproducibility.

---

## Module patterns

### Module structure

```nix
{ config, lib, pkgs, ... }:

let
  cfg = config.services.myapp;
in {
  options.services.myapp = {
    enable = lib.mkEnableOption "My application";
    package = lib.mkPackageOption pkgs "myapp" { };
    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/myapp";
      description = "Directory for application data";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.myapp = {
      description = "My Application";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/myapp serve";
        WorkingDirectory = cfg.dataDir;
        DynamicUser = true;
        Restart = "on-failure";
      };
    };
  };
}
```

### Key patterns

- **`cfg` alias**: Always alias `config.services.myapp` at the top of the module
- **`mkIf` + `mkMerge`**: Conditional blocks for feature toggling
- **Module composition**: Split config into logical files, use `imports` to compose

### Key module functions

| Function | Priority | Purpose |
|----------|----------|---------|
| `lib.mkDefault` | 1000 | Set default value (overridable) |
| `lib.mkForce` | 50 | Force a value |
| `lib.mkIf cond { ... }` | | Conditional config |
| `lib.mkMerge [ ... ]` | | Merge multiple config fragments |
| `lib.mkEnableOption "desc"` | | Boolean option with default `false` |
| `lib.mkPackageOption pkgs "name" {}` | | Package option with default from pkgs |

---

## Derivation and packaging

### Rust with crane

Crane is the preferred Rust packaging framework for Nix. It splits builds into dependency and source phases for maximum caching.

- Two-phase build: `buildDepsOnly` (cached) then `buildPackage` (reuses artifacts)
- `cleanCargoSource` filters source to only Rust-relevant files
- `commonArgs` pattern: share args between dep and full builds

| Option | Verdict | Reason |
|--------|---------|--------|
| `crane` | Use this | Two-phase build, best caching, actively maintained |
| `buildRustPackage` (nixpkgs) | Avoid | Single-phase, rebuilds deps on every source change |
| `naersk` | Avoid | Less composable, smaller community |

### Development shell

```nix
devShells.default = pkgs.mkShell {
  inputsFrom = [ myPackage ];  # Inherit build deps
  packages = with pkgs; [
    rust-analyzer
    cargo-nextest
    nixfmt
  ];
};
```

### Environment decision tree

Choose the right environment builder for the task.

| Builder | Use when | Trade-offs |
|---------|----------|------------|
| `mkShell` | Development shells, CI environments | Lightweight. Does not produce a derivation output. Cannot be installed or deployed. |
| `stdenv.mkDerivation` | Building packages for installation or deployment | Full derivation lifecycle (unpack, patch, configure, build, install). Heavier to iterate on. |
| `buildFHSEnv` | Wrapping binaries that assume FHS paths (`/usr/lib`, `/usr/bin`) | Creates a lightweight FHS-compatible sandbox. Necessary for proprietary tools, pre-built binaries, and some language toolchains that hardcode paths. Adds runtime overhead from the namespace mount. |

**`mkShell` is not a subset of `mkDerivation`.** `mkShell` skips build phases entirely and only sets up environment variables. It is purpose-built for `nix develop` / `nix-shell`. Do not try to build a `mkShell` derivation — it will produce an empty output.

**Use `buildFHSEnv` only when the binary cannot be patched.** For open-source software, prefer patching with `autoPatchelfHook` or `patchelf` directly. WHY: `buildFHSEnv` hides the FHS assumption rather than fixing it, and the namespace mount adds startup latency and complicates debugging.

```nix
# FHS environment for a proprietary tool
pkgs.buildFHSEnv {
  name = "vendor-tool";
  targetPkgs = pkgs: with pkgs; [ zlib openssl ];
  runScript = "./vendor-tool";
}
```

---

## Overlays

Overlays customize nixpkgs without forking it. An overlay is a function that takes two arguments (`final`, `prev`) and returns an attrset of additions or modifications merged into the package set.

### Overlay structure

```nix
# final: the fully resolved package set (use for dependencies)
# prev: the package set before this overlay (use for the base package being modified)
final: prev: {
  myapp = final.callPackage ./pkgs/myapp { };

  ffmpeg = prev.ffmpeg.override {
    withFdk = true;
  };
}
```

**Use `final` for dependencies, `prev` for the package being modified.** WHY: Using `prev` for dependencies can reference stale versions. Using `final` for the base package causes infinite recursion since the package depends on itself.

### Applying overlays

```nix
# In a flake — explicit import with overlays list
pkgs = import nixpkgs {
  inherit system;
  config = {};
  overlays = [ self.overlays.default ];
};

# In a NixOS configuration — via nixpkgs.overlays option
nixpkgs.overlays = [ self.overlays.default ];
```

**Always apply overlays via the `overlays` parameter or `nixpkgs.overlays` option.** WHY: Ad-hoc `pkgs.extend` calls create package sets that diverge from the one NixOS modules see, causing subtle version mismatches.

### Overlay patterns

**One overlay per concern.** Split custom packages, version pins, and patches into separate overlays. WHY: Monolithic overlays are hard to toggle, hard to debug, and create unnecessary rebuilds when one part changes.

**Export overlays from the flake.**

```nix
# flake.nix outputs
overlays.default = final: prev: {
  myapp = final.callPackage ./pkgs/myapp { };
};
```

WHY: Consumers of your flake can compose your overlay with their own package set instead of depending on your specific nixpkgs pin.

**Never use `rec` inside an overlay body.** Use `final` to reference sibling packages. WHY: `rec` binds at definition time and ignores later overlays, defeating the entire overlay composition model.

**Pin `config = {}; overlays = [];` on any `import nixpkgs` call that itself receives overlays.** WHY: Without this, system-level nixpkgs config and overlays leak in, making the result non-reproducible and potentially applying overlays twice.

### Overlay ordering

Overlays apply left to right. Later overlays see modifications from earlier ones via `final`. If overlay B depends on packages added by overlay A, list A first.

---

## Cross-compilation

Nix separates the concept of *where code runs* from *where code is built*. This makes cross-compilation a first-class operation rather than an afterthought.

### Platform terminology

| Term | Meaning | Example |
|------|---------|---------|
| `buildPlatform` | Machine running the compiler | `x86_64-linux` |
| `hostPlatform` | Machine running the compiled binary | `aarch64-linux` |
| `targetPlatform` | Machine the compiled binary generates code for (compilers only) | `riscv64-linux` |

Most packages only care about `buildPlatform` and `hostPlatform`. `targetPlatform` matters only for toolchains (GCC, LLVM, binutils).

### Cross-compiling with nixpkgs

```nix
# Import nixpkgs with crossSystem set
pkgsCross = import nixpkgs {
  localSystem = "x86_64-linux";
  crossSystem = "aarch64-linux";
  config = {};
  overlays = [];
};

# Or use the pre-configured cross package sets
pkgs.pkgsCross.aarch64-multiplatform.hello
pkgs.pkgsCross.raspberryPi.hello
pkgs.pkgsCross.riscv64.hello
```

**Use `pkgsCross` attribute sets for standard targets.** WHY: They are pre-configured with the correct toolchain, sysroot, and platform flags. Manual `crossSystem` is needed only for non-standard targets.

### Spliced package sets

In a cross-compilation context, nixpkgs provides spliced package sets that automatically select the right variant:

| Reference | Resolves to | Use for |
|-----------|-------------|---------|
| `pkgs.pkg` | Host package | Runtime dependencies |
| `pkgs.buildPackages.pkg` | Build package | Build-time tools (code generators, compilers) |
| `pkgs.__targetPackages.pkg` | Target package | Rare — only for building toolchains |

**Use `nativeBuildInputs` for build-time tools, `buildInputs` for runtime dependencies.** WHY: Nix uses this distinction to select the correct spliced package. Putting a build tool in `buildInputs` during cross-compilation pulls in the wrong architecture binary, and the build fails or silently produces a broken result.

### Cross-compilation in a flake

```nix
# Expose a cross-compiled package alongside the native one
packages.x86_64-linux = let
  pkgs = import nixpkgs { system = "x86_64-linux"; config = {}; overlays = []; };
  pkgsAarch64 = import nixpkgs {
    localSystem = "x86_64-linux";
    crossSystem = "aarch64-linux";
    config = {};
    overlays = [];
  };
in {
  default = pkgs.callPackage ./. { };
  aarch64 = pkgsAarch64.callPackage ./. { };
};
```

### Musl static builds

Static linking with musl produces fully self-contained binaries with no glibc dependency. This is the standard approach for container images and single-binary deployment.

```nix
# Static musl build for x86_64
pkgs.pkgsStatic.callPackage ./. { }

# Cross-compile a static aarch64 binary on x86_64
let
  pkgsCross = import nixpkgs {
    localSystem = "x86_64-linux";
    crossSystem = {
      config = "aarch64-unknown-linux-musl";
      isStatic = true;
    };
    config = {};
    overlays = [];
  };
in pkgsCross.callPackage ./. { }
```

**Use `pkgsStatic` for same-architecture static builds.** WHY: `pkgsStatic` is a pre-configured package set where `stdenv` targets musl and sets static linking flags. Manual `RUSTFLAGS` or `LDFLAGS` overrides are fragile and miss transitive dependencies.

**Static builds break packages that dlopen.** Libraries loaded at runtime via `dlopen` (NSS, locale data, some database drivers) do not work with static musl. If the binary needs dynamic loading, use glibc with `--static` selectively or accept dynamic linking for those dependencies.

### Making a derivation cross-compatible

**Split `buildInputs` and `nativeBuildInputs` correctly.** This is the single most common cross-compilation failure.

```nix
{
  nativeBuildInputs = [ pkg-config cmake ];  # Runs on build machine
  buildInputs = [ openssl zlib ];            # Links into the final binary
}
```

**Do not hardcode architecture paths or compiler names.** Use variables from the stdenv toolchain (`$CC`, `$AR`, `$STRIP`). WHY: Hardcoded `gcc` or `/usr/lib` bypasses the cross toolchain and produces native binaries instead of cross-compiled ones.

**Test cross-compilation in CI.** Add a `nix build .#aarch64` check even if your deploy target is the same architecture today. WHY: Cross-compilation correctness degrades silently. A build that works natively can fail when cross-compiled due to impure build scripts, and you will not find out until you need it.

---

## Lazy evaluation debugging

Nix is lazily evaluated: no value is computed until it is needed. This is powerful (unused code costs nothing) but creates debugging challenges that are unlike any strict language.

### Core mental model

**Nix values are thunks until forced.** A thunk is an unevaluated expression. Errors inside a thunk do not surface until something demands the value. This means:

- An attrset can contain a key whose value is an error, and accessing other keys works fine
- A list can contain a `throw` element, and `builtins.length` still returns the correct count
- An `assert` inside an unused `let` binding never fires

### Debugging tools

**`builtins.trace`**: Print a value during evaluation and return the second argument.

```nix
builtins.trace "evaluating foo" foo
# Prints "evaluating foo" to stderr, returns value of foo

# Trace an attrset (forces it for printing)
builtins.trace (builtins.toJSON { inherit x y; }) result
```

WHY `builtins.trace` over `lib.debug.traceVal`: `builtins.trace` takes two arguments (message, return value), giving explicit control over what is printed vs. returned. `lib.debug.traceVal` is a convenience wrapper. Use `builtins.trace` when you need to trace one value and return a different one.

**`builtins.deepSeq`**: Force full evaluation of a value (recursively).

```nix
# Force evaluation of the entire attrset, surface any hidden errors
builtins.deepSeq myAttrset myAttrset
```

WHY: Normal evaluation only forces the values actually demanded by the build. `deepSeq` forces everything, which reveals errors hiding in unused branches. Use it as a diagnostic tool, not in production derivations.

**`builtins.tryEval`**: Catch evaluation errors without crashing.

```nix
builtins.tryEval (throw "broken")
# => { success = false; value = false; }

builtins.tryEval 42
# => { success = true; value = 42; }
```

WHY: Useful for probing which values in a set are evaluable. Does NOT catch errors inside `builtins.deepSeq` or derivation builds.

### Common lazy evaluation traps

**Infinite recursion from self-reference.** Nix detects direct cycles (`x = x`) but not all indirect ones. The error message `infinite recursion encountered` gives no stack trace by default.

```bash
# Get a stack trace on infinite recursion
nix eval --show-trace .#problematicValue
```

**Always pass `--show-trace` when debugging evaluation errors.** WHY: Without it, Nix shows only the final error. With it, you get the full chain of file locations and function calls that led to the failure.

**Errors inside `builtins.map` or `builtins.filter` are deferred.** The list is constructed lazily. Errors surface only when the specific element is forced.

```nix
# This succeeds — the broken element is never accessed
let
  xs = builtins.map (x: if x == 2 then throw "boom" else x) [ 1 2 3 ];
in builtins.head xs  # => 1

# This fails — element at index 1 is forced
let
  xs = builtins.map (x: if x == 2 then throw "boom" else x) [ 1 2 3 ];
in builtins.elemAt xs 1  # => error: boom
```

**`//` (merge) does not force values.** Merging two attrsets does not evaluate the values on either side. An error in a value from the left side persists silently if nothing accesses it, even after the merge.

**Attribute selection is the primary forcing mechanism.** Accessing `x.foo` forces the thunk for `foo` (but not `x.bar`). Build systems force attributes by demanding derivation outputs. Understanding what forces what is the key to understanding when errors appear.

### Debugging workflow

1. **Reproduce with `--show-trace`.** `nix eval --show-trace .#attr` or `nix build --show-trace .#pkg`.
2. **Isolate the thunk.** Use the REPL (`nix repl .`) to interactively access attributes and find which key triggers the error.
3. **Insert `builtins.trace` at the boundary.** Trace function arguments at the point where the value enters the failing expression.
4. **Force with `builtins.deepSeq` to smoke-test.** If you suspect hidden errors in an attrset, force it fully to surface them all at once.
5. **Check for `//` masking.** If a value "should" be set but is not, verify it was not silently replaced by a shallow merge.

---

## Dependency auditing

Nix closures can be large. Auditing what a derivation actually pulls in prevents bloated images, unexpected runtime dependencies, and known-vulnerable packages shipping in production.

### Closure analysis

```bash
# Show the full closure (all runtime dependencies) of a package
nix path-info -rsSh .#mypackage

# Compare closures between two versions or configurations
nix-diff /nix/store/<hash-a>-mypackage /nix/store/<hash-b>-mypackage

# List closure as a dependency tree
nix-store --query --tree $(nix build .#mypackage --print-out-paths)
```

**Use `nix path-info -rsSh` to check closure size before deploying.** WHY: A single misplaced runtime dependency (GCC toolchain, Python interpreter, X11 libraries) can inflate a container image from 50MB to 2GB. Closure size is invisible until you measure it.

**Use `nix-diff` when a closure size changes unexpectedly.** `nix-diff` shows exactly which derivations changed and why — new dependencies, version bumps, or build flag differences. It operates on store paths, not source, so it catches transitive changes that diff-on-source misses.

### Reducing closure size

**Move build-only dependencies to `nativeBuildInputs`.** Packages in `buildInputs` propagate into the runtime closure. Build tools (compilers, code generators, `pkg-config`) in `buildInputs` bloat the closure for no benefit.

**Use `removeReferencesTo` for stubborn references.** Some build systems embed store paths in binaries (rpath, embedded config). If a reference is not needed at runtime:

```nix
postInstall = ''
  remove-references-to -t ${pkgs.stdenv.cc} $out/bin/myapp
'';
```

WHY: The Nix garbage collector and closure computation follow store path references. A single stray reference to `gcc` pulls in the entire toolchain as a runtime dependency.

### Vulnerability scanning

```bash
# Scan a closure for known CVEs using vulnix
vulnix $(nix build .#mypackage --print-out-paths)

# Check a specific store path
vulnix /nix/store/<hash>-openssl-3.1.4
```

**Run `vulnix` in CI against production closures.** WHY: Nix pins exact package versions. Unlike rolling distributions, a pinned `flake.lock` does not receive security patches automatically. Scanning detects when a pinned version has known vulnerabilities, prompting a `nix flake update` for the affected input.

**`vulnix` matches store paths against the NVD (National Vulnerability Database).** False positives are common when nixpkgs backports patches without bumping the version number. Check the nixpkgs commit log for the package before acting on a CVE report.

---

## Anti-patterns

### `rec { ... }`: avoid recursive attrsets

Use `let ... in` instead. `rec` creates easy infinite recursion by shadowing.

### `with` at file scope: pollutes namespace

Use explicit `pkgs.X` prefixing. `with` is acceptable only in small list contexts where scope is obvious.

### Lookup paths (`<nixpkgs>`): non-reproducible

Depends on `$NIX_PATH` environment variable. Pin via flake input instead.

### Unpinned `import nixpkgs {}`

Always set `config = {}; overlays = [];` explicitly. System files can influence the result otherwise.

### Shallow merge surprise with `//`

Nested attrsets are replaced entirely. Use `lib.recursiveUpdate` for deep merges.

### Bare uRLs

```nix
# Bad (deprecated syntax)
inputs.nixpkgs.url = https://github.com/NixOS/nixpkgs;

# Good
inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
```

### FHS assumptions

NixOS does not follow the Filesystem Hierarchy Standard. No `/usr/bin/`, no global `/lib/`. Use `buildFHSEnv` to wrap non-Nix binaries.

### System-independent outputs inside `eachDefaultSystem`

```nix
# Bad: nixosModules ends up under a system key
flake-utils.lib.eachDefaultSystem (system: {
  nixosModules.default = ...;  # Wrong!
});

# Good: merge system-specific and system-independent separately
flake-utils.lib.eachDefaultSystem (system: {
  packages.default = ...;
}) // {
  nixosModules.default = ...;  # Top level
}
```

---

## Conventions

1. **One flake.** Everything flows from `flake.nix`. No channel-based config, no `NIX_PATH`.
2. **Commit `flake.lock`.** It IS reproducibility.
3. **`.follows` on all transitive nixpkgs.** No version divergence.
4. **`config = {}; overlays = [];`** when importing nixpkgs. No impure system state.
5. **Crane for Rust.** Two-phase build. Always split deps from source.
6. **nixfmt for formatting.** No debate. Run in CI.
7. **Explicit > implicit.** `pkgs.git` over `with pkgs; [ git ]`.
8. **`let ... in` over `rec`.** Always.
9. **`specialArgs`** to pass flake inputs to modules. Not `_module.args`.
10. **Checks gate CI.** `nix flake check` must pass.
11. **No lookup paths.** No `<nixpkgs>`. No `$NIX_PATH` dependencies.
12. **`final` for deps, `prev` for the package being modified** in overlays. No `rec` in overlay bodies.
13. **One overlay per concern.** Export from the flake for composability.
14. **`nativeBuildInputs` for build tools, `buildInputs` for runtime deps.** Non-negotiable for cross-compilation correctness.
15. **`pkgsStatic` for musl static builds.** Do not set linker flags manually.
16. **`buildFHSEnv` only for unpatchable binaries.** Prefer `autoPatchelfHook` for open-source software.
17. **`--show-trace` on all debugging.** Never debug evaluation errors without it.
18. **Audit closure size before deploying.** `nix path-info -rsSh` catches accidental bloat.
19. **`vulnix` in CI on production closures.** Pinned versions do not auto-update for security.

---

## Tooling

| Tool | Purpose |
|------|---------|
| `nix` | Package manager + language evaluator |
| `nixfmt` | Official formatter (RFC 166) |
| `nix repl` | Interactive REPL for testing expressions |
| `nix flake check` | Validate flake schema + run checks |
| `nix flake show` | Display flake outputs |
| `nixd` or `nil` | LSP for editor integration |
| `statix` | Nix linter (catches anti-patterns) |
| `deadnix` | Find unused code in Nix files |
| `nix-tree` | Visualize dependency tree |
| `nix-diff` | Compare closures to find what changed between builds |
| `vulnix` | Scan Nix closures for known CVEs |
| `nix path-info` | Inspect closure size and dependencies |
