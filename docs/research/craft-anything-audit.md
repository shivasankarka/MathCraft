# ArtCraft / Craft Anything audit (2026-11-07)

Method: web search and fetch of github.com/storytold, craftingapps.org, and shallow clones of `craft-anything`
(v0.4.0) and `figcraft`. Not built, not run. Star/fork counts come from a summarising fetch, treat as approximate.

## What it is
- ArtCraft team ("storytold" org on GitHub): 16 open-source Rust "Crafting Apps" (PhotoCraft, VectorCraft, FilmCraft, LightCraft, PdfCraft, EffectCraft, DesignCraft, WordCraft, SoundCraft, DeckCraft, CADCraft, GridCraft, FigCraft, TextureCraft, MaterialCraft, Craft Anything). GridCraft (spreadsheet) has no public repo that I found. **No math/CAS/notebook app exists in the suite.**
- Craft Anything: clean-room engine that studies a website/app/game, has a coding agent (Claude Code first) rebuild it, diffs the result against the original and refines. Desktop (egui), CLI and MCP server on one engine. Early alpha, v0.4.0, released 2026-11-01. MIT OR Apache-2.0; ArtCraft name/logos trademarked. Claims 320 tools (a social post said 284).
- ~216k lines Rust across 13 crates in craft-anything; FigCraft ~355k lines.

## Architecture to copy
Layered crates L0 to L6 enforced by `cargo xtask layers`: evidence/formats (L0) > disasm/js (L1) > analysis/platform (L2) > engines (L3) > engine (L4) > ai/mcp/setup (L5) > ui-egui (L6, only crate allowed egui/eframe/winit).

Golden rules from `AGENTS.md`: never crash on hostile input; everything is a `ToolSpec` (UI, CLI, MCP, AI agent dispatch by id; params via schemars); structured evidence results; honest declared side effects; L0 to L2 never break WASM; thin UI with serde-data state and theme tokens; no secrets; `unsafe_code = "forbid"`; external programs only through one process wrapper with deadlines; custom Mutex that detects deadlocks; nothing slow on the UI thread; versioned persisted files; extension through traits not enum arms; typed data tables not string matches; roadmap with ids/owners/claims, `cargo xtask pending`; one roadmap item per PR; `docs/pitfalls.md` for mistakes worth not repeating.

Stack: egui/eframe 0.36 (FigCraft 0.37 with a forked egui + winit), wgpu (Craft Anything vendors a patched wgpu-hal), tiny-skia and kurbo in FigCraft, rayon, proptest, egui_kittest for UI tests. Release: thin LTO, separate `web-release` profile (opt-level s, fat LTO) for the WASM viewer. Packages for macOS (signed, notarized), Windows MSI, Linux AppImage/Flatpak/deb/rpm, FreeBSD, WASM.

## Policies relevant to us
- Licences: permissive only, MPL-2.0 by named exception, never copyleft; `cargo deny` + `cargo-about` (`deny.toml`, `docs/licences.md`). Dependencies from unknown git sources are denied.
- Shared standards live in `storytold/craft-contrib`, CI in `storytold/craft-common-actions`; repos do not share code with each other.
- House rules (from FigCraft `AGENTS.md`): a person is accountable for what an agent opens; claim before coding; narrow tested fixes; main changes only through PRs; no agent attribution in PRs.
- Craft Anything's own rule: original work only, never copy code from other projects.

## Fit and risks for MathCraft
- Reuse: layering, tool registry, headless-first, never-crash discipline, licence tooling, roadmap process, packaging.
- Not reusable: their engine is reverse engineering; no math, no notebook, no typeset math.
- Craft Anything is early alpha; many items verified only on synthetic fixtures (`docs/STATUS.md`). Copy patterns, do not depend on it.
- Open (Q9): whether Craft Anything can drive a greenfield project build. Its docs describe study/build/check/ready for existing targets; STATUS mentions new-project toolchains for Python, Go, .NET, Maven, Gradle, Make; Rust/Electron/Godot keep launch priority. Unverified for a new Rust workspace.
- Legal caution: do not use a reverse-engineering workflow on Mathematica itself.
