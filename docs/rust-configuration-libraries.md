# Rust config crates for cross-platform desktop apps

**For your specific requirements — CLI path override, OS-specific default paths, TOML, layered overrides, and auto-bootstrapping on first run — no single crate checks every box.** The two best paths are a **`confy` + `clap`** stack if you prioritize zero boilerplate and the auto-bootstrap requirement, or a **`figment` + `directories` + `clap`** stack if you need full Spring-Boot-style layered precedence with env vars. The DIY approach (`clap` + `directories` + `serde` + `toml`) is also genuinely idiomatic for desktop apps and only costs ~60 LOC.

In Spring Boot terms: **`figment` is the closest analog to Spring's PropertySource ordering** (defaults → application.yml → env → CLI, with provenance tracking), while **`confy` is closer to `spring.config.location` plus `@ConfigurationProperties` with auto-persisted defaults** but without the layering. `config-rs` is the most "12-factor" but has a weaker ergonomics story. The rest — `twelf`, `config-file`, `confique`, `confik` — are either stagnant, too niche, or don't add enough value over the big three to recommend for a new desktop project in 2026.

## How the contenders stack up at a glance

| Requirement | **confy** | **config-rs** | **figment** | **twelf** | **DIY (clap+directories+serde+toml)** |
|---|---|---|---|---|---|
| CLI `-C/--config` override | Manual (trivial) | Manual (trivial) | Manual (trivial) | Native via `Layer::Clap` + `clap_args()` | Native |
| OS-specific default paths | **Built-in** (via `etcetera`) | None — bring `directories` | None — bring `directories` | None — bring `directories` | Explicit with `directories` |
| TOML | Default feature | Feature-gated | First-class provider | Feature-gated (old toml 0.5) | Direct via `toml` crate |
| Layered (defaults→file→env→CLI) | **No** (explicit non-goal) | **Yes** (builder) | **Yes** (`.merge`/`.join`) | **Yes** (`Layer` enum) | Manual, explicit |
| Env-var overrides (prefix) | No | `Environment::with_prefix` | `Env::prefixed(...).split(...)` | `Layer::Env` (via `envy`) | Via `envy` or `std::env::var` |
| Auto-create defaults on first run | **Yes** (via `Default`) | No | No | No | Manual (~5 lines) |
| Writes config back | Yes (`store`) | **No** (documented) | No | No | Yes (`toml::to_string_pretty`) |
| Active maintenance (Apr 2026) | Slow but alive, v2.0 Oct 2025 | Very active, monthly | Stable but stale (no release since Apr 2024) | **Stagnant** (~2 yrs) | N/A |
| crates.io downloads | ~2.1M | ~79.7M | ~23.7M | small | — |
| GitHub stars | ~1,000 | ~3,100 | ~705 | ~122 | — |

## Why confy is the closest out-of-the-box fit — with one big caveat

**`confy` uniquely solves requirements 2 (OS paths) and 5 (default bootstrapping) natively.** A single call — `confy::load("my-app", None)` — resolves the OS-specific config path, reads the file if present, or writes `MyConfig::default()` to disk and returns it. Since v2.0.0 (October 2025) it uses the **`etcetera`** crate internally and offers two strategies via `confy::change_config_strategy()`:

- **`ConfigStrategy::App`** (the new default) — XDG-style on all platforms: `~/.config/<app>` on Linux **and macOS**, `%APPDATA%\<app>\config` on Windows.
- **`ConfigStrategy::Native`** — macOS-native `~/Library/Application Support/rs.<app>`; same Linux/Windows behavior.

**Important**: for a GUI desktop app, you almost certainly want `Native`; the v2.0.0 default changed macOS behavior to XDG, which is a breaking change from v1.x. Call `change_config_strategy(ConfigStrategy::Native)` explicitly at startup.

The caveat is that **confy has explicit non-goals** (stated by its author on GitHub issue #3): no environment variables, no layering, no CLI integration. If requirement 4 (layered precedence with env var overrides) is more than a "nice-to-have," confy forces you to hand-roll that part.

### confy + clap code example

```toml
[dependencies]
confy  = "2.0"
serde  = { version = "1", features = ["derive"] }
clap   = { version = "4", features = ["derive"] }
```

```rust
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    /// Override config file path (otherwise OS default is used)
    #[arg(short = 'C', long = "config")]
    config: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    theme: String,
    window_width: u32,
    window_height: u32,
    api_key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { theme: "dark".into(), window_width: 1280,
               window_height: 720, api_key: String::new() }
    }
}

fn main() -> Result<(), confy::ConfyError> {
    // Desktop GUI → native macOS path (~/Library/Application Support/…)
    confy::change_config_strategy(confy::ConfigStrategy::Native);
    let cli = Cli::parse();

    let mut cfg: AppConfig = match &cli.config {
        Some(p) => confy::load_path(p)?,       // arbitrary path, still auto-creates
        None    => confy::load("my-app", None)?, // OS default + bootstrap
    };

    // Manual env override — confy doesn't do this for you
    if let Ok(theme) = std::env::var("MY_APP_THEME") { cfg.theme = theme; }

    println!("Loaded from {:?}: {:?}",
             confy::get_configuration_file_path("my-app", None)?, cfg);
    Ok(())
}
```

## Why figment is the strongest fit if you actually need layering

**`figment` (from Sergio Benitez of Rocket fame)** gives you true Spring-style precedence via `Figment::from(...).merge(...).merge(...)`, tracks the **provenance of every value** (error messages tell you the exact file, key, and provider), and handles TOML/JSON/YAML/Env uniformly. It's the most Spring-like option in the Rust ecosystem.

The cost is that **you wire up the path resolution and first-run bootstrap yourself** — about 10 lines with `directories` and `std::fs`. Figment also has **no official clap provider**; the idiomatic workaround is to make your clap struct `Serialize` (with `#[serde(skip_serializing_if = "Option::is_none")]`) and pass it as `Serialized::defaults(cli)` as the last layer. Release cadence is a mild concern — **no release since 0.10.19 in April 2024** — but the code is stable and still powers every Rocket app.

### figment + directories + clap code example (full layered precedence)

```toml
[dependencies]
figment      = { version = "0.10", features = ["toml", "env"] }
directories  = "5"
clap         = { version = "4", features = ["derive"] }
serde        = { version = "1", features = ["derive"] }
toml         = "0.8"
anyhow       = "1"
```

```rust
use clap::Parser;
use directories::ProjectDirs;
use figment::{Figment, providers::{Format, Toml, Env, Serialized}};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Parser, Debug, Serialize)]
struct Cli {
    #[arg(short = 'C', long = "config")]
    #[serde(skip)]
    config: Option<PathBuf>,

    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig { theme: String, port: u16 }
impl Default for AppConfig {
    fn default() -> Self { Self { theme: "dark".into(), port: 8080 } }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Requirement 1 + 2: CLI override, else OS default via directories
    let path = cli.config.clone().unwrap_or_else(|| {
        let pd = ProjectDirs::from("com", "FooCorp", "MyApp").unwrap();
        pd.config_dir().join("config.toml")
    });

    // Requirement 5: bootstrap default on first run
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, toml::to_string_pretty(&AppConfig::default())?)?;
    }

    // Requirement 4: layered precedence (later overrides earlier)
    let cfg: AppConfig = Figment::from(Serialized::defaults(AppConfig::default()))
        .merge(Toml::file(&path))
        .merge(Env::prefixed("MYAPP_").split("__"))  // MYAPP_PORT, MYAPP_DB__HOST
        .merge(Serialized::defaults(&cli))           // CLI wins
        .extract()?;

    println!("{:?}", cfg);
    Ok(())
}
```

## The DIY stack is genuinely idiomatic and probably what you should ship

Browse real Rust desktop apps (Tauri, iced, egui, gtk4-rs ports) and you'll find the **dominant 2025-2026 pattern is `clap` + `directories` + `serde` + `toml`** with ~60 LOC of explicit glue. This is what the Rust CLI working group's book recommends for most cases, and it's what authors like BurntSushi (ripgrep) tend to ship. Reasons this pattern dominates:

- **Zero magic, zero proc-macro surprises, auditable in five minutes.**
- You own the first-run UX — better error messages, migration story, comments in the default file.
- You can write back to the file (both `config-rs` and `figment` are read-only).
- Only 4 small dependencies; no unused YAML/INI/JSON parsers pulled in.
- Env var overrides are literally `std::env::var("MYAPP_THEME").ok()` — and `envy` adds flat serde-struct env parsing for ~10 more LOC.

Use `directories::ProjectDirs::from("com", "FooCorp", "MyApp").config_dir()` and you get the correct path on all three OSes automatically: `~/.config/myapp` on Linux, `C:\Users\Alice\AppData\Roaming\FooCorp\MyApp\config` on Windows, `~/Library/Application Support/com.FooCorp.MyApp` on macOS. Swap to **`etcetera::choose_app_strategy`** if you want explicit XDG on macOS (common for CLI-leaning apps where users hand-edit the config) or `choose_native_strategy` for Apple-standard paths on GUI apps.

A complete working DIY implementation covering all six of your requirements (CLI override, OS paths, TOML, layered with env overrides, first-run bootstrap, cross-platform) fits in ~100 LOC — see the pattern at the end of this section.

### DIY skeleton (the pragmatic recommendation for most desktop apps)

```rust
use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

#[derive(Parser)]
struct Cli {
    #[arg(short = 'C', long = "config")]
    config: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig { theme: String, log_level: String, window: Window }
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct Window { width: u32, height: u32 }

impl Default for AppConfig {
    fn default() -> Self {
        Self { theme: "system".into(), log_level: "info".into(),
               window: Window { width: 1280, height: 800 } }
    }
}
impl Default for Window { fn default() -> Self { Self { width: 1280, height: 800 } } }

fn resolve_path(cli: &Cli) -> Result<PathBuf> {
    if let Some(p) = &cli.config { return Ok(p.clone()); }
    let pd = ProjectDirs::from("com", "FooCorp", "MyApp")
        .context("no home dir")?;
    Ok(pd.config_dir().join("config.toml"))
}

fn load_or_init(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap())?;
        let def = AppConfig::default();
        fs::write(path, toml::to_string_pretty(&def)?)?;
        eprintln!("Created default config at {}", path.display());
        return Ok(def);
    }
    let raw = fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}

fn apply_env(cfg: &mut AppConfig) {
    if let Ok(v) = std::env::var("MYAPP_THEME")     { cfg.theme = v; }
    if let Ok(v) = std::env::var("MYAPP_LOG_LEVEL") { cfg.log_level = v; }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = resolve_path(&cli)?;
    let mut cfg = load_or_init(&path)?;
    apply_env(&mut cfg);
    println!("{:?}", cfg);
    Ok(())
}
```

## Why I'd steer you away from twelf, config-rs, and config-file

**`twelf`** looks attractive on paper — it's the only crate with native clap integration via `Conf::clap_args()` and `Layer::Clap(matches)` — but it's **essentially stagnant**: last release late 2023, still pinned to `toml 0.5`, `serde_yaml 0.8`, `thiserror 1`, and has an open bug (#15) where clap-derive interop is broken. For a new 2026 project this is a meaningful dependency-hygiene risk.

**`config-rs`** is the most active and most downloaded (79M), and its layered-builder API is powerful, but it has three big strikes against it for a **desktop** app: (1) **no default OS path resolution** — you still need `directories`; (2) **no file write-back** (documented limitation — you can never persist changes); (3) **no first-run bootstrap**. The maintainers themselves have an open issue (#111/#321) acknowledging the crate is "complex to use, complex to develop" and exploring a redesign. It's a better fit for 12-factor servers than for apps that *own* their own config file.

**`config-file`** is unmaintained (last release April 2022) and offers no layering, env vars, or CLI integration — **don't use it**.

**`confique` and `confik`** are reasonable modern alternatives with serde-derive APIs and layered support, but they don't add enough value over figment to justify the smaller ecosystems and fewer eyeballs. Mention them if you want; don't lead with them.

## Ranked recommendation and final verdict

1. **Confy + clap** — if your priorities are "OS paths and bootstrap handled automatically, minimum code, one format." You sacrifice layering/env vars. Best for simple GUI apps where the app writes its own config.
2. **DIY (clap + directories + serde + toml)** — the **pragmatic default** for most Rust desktop apps. ~100 LOC, zero magic, full control, all six requirements satisfied, and you can always upgrade to figment later without changing the on-disk format.
3. **Figment + directories + clap** — if layering and env-var precedence are firm requirements (your scenario explicitly lists env overrides), figment is the cleanest way to express `defaults → file → env → CLI` in Rust today, and its error provenance is the single best in the ecosystem. Closest to Spring Boot's `PropertySource` mental model.
4. **Config-rs + directories + clap** — pick only if you're building something server-adjacent with heavy 12-factor/multi-env deployment needs and you don't need to write the config file back.
5. **Twelf / confique / confik / config-file** — not recommended for a new 2026 project.

### Combining libraries is the idiomatic pattern, not a workaround

To answer your explicit question: **yes, combining libraries is the norm, not the exception.** No crate in the ecosystem ships all six features. The standard combos are (a) `confy + clap` when auto-bootstrap matters more than layering, (b) `figment + directories + clap` when layering matters more, and (c) plain `clap + directories + serde + toml` when neither of the above quite fits and you want to avoid framework lock-in. Whichever you pick, `directories` (or `etcetera`) supplies the per-OS paths, `clap` supplies the `-C/--config` flag, and `serde + toml` supply the parse layer — these are stable foundations that outlast any particular config-framework fashion.

## Conclusion

Your six requirements straddle two philosophies: **"the app owns its config file"** (favors confy / DIY, with write-back and auto-bootstrap) and **"the environment dictates config"** (favors figment / config-rs, with layered precedence). Because you listed env-var overrides as only a "nice-to-have" and auto-bootstrap as an explicit must-have, the evidence points to **confy as your shortest path to a working solution** and the **DIY stack as your most flexible and maintainable one**. If you discover later that you need proper layered precedence — or you want Spring-like provenance tracking in your error messages — **figment is a low-friction upgrade** since both confy and DIY use the same `serde` structs and `toml` format on disk.

One non-obvious insight: the Rust desktop-app community has quietly converged on the DIY pattern precisely because config frameworks optimize for server-style 12-factor scenarios that don't match how desktop apps actually use their config (user-editable files the app both reads *and* writes). Don't assume you need a framework just because you would in Spring Boot — the ~60 LOC DIY version is often more readable and more correct for this domain.