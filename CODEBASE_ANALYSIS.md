# Wiggler codebase analysis and improvement plan

_Review and implementation date: 2026-09-28. Scope: Rust source, manifest, lockfile, assets, Inno Setup script, and the local v1.0.0 PRD. The PRD is currently untracked in Git. The baseline findings below describe the code before this hardening pass; the status and verification sections describe the updated build._

## Executive assessment

Wiggler has a suitably small dependency and module footprint for its purpose. The pure movement and state logic is separated from Windows calls, the default profile and delay match the PRD, settings are stored locally, and injected events are filtered with `LLMHF_INJECTED`. A per-user installer script and a built installer are present.

The baseline build was **not ready to call v1.0.0 complete**: normal shutdown had a double free, input monitoring could fail without stopping injection, and several motion, persistence, and lifecycle requirements were incomplete. The hardening pass fixed these source defects in place, retained the small architecture, built an installer, and verified core installed behaviors. Physical device override, some lifecycle transitions, multi-monitor behavior, and a long soak remain unverified, so the full PRD release gate is still open.

## Current implementation status

| Area | Completed in this pass | Remaining proof or limitation |
| --- | --- | --- |
| Shutdown and startup | Removed the UI double free; the tray and hook now report readiness; the hook thread joins before its event closes. | Repeated rapid shutdown with real mouse activity has not been stress-tested. |
| Input safety | The runtime stops injection when hook readiness is lost, rechecks genuine activity immediately before injection, and requires the hook to acknowledge injected movement; a missing acknowledgement triggers hook reset. | A physical event that arrives during `SendInput` can still race with that call. Windows can silently remove a low-level hook; acknowledgement limits an unnoticed loss to the first attempted motion but cannot prove the hook alive before that attempt. Real mouse and touchpad timing must be measured. |
| Motion | Brownian starts at rest; Lissajous has slow phase drift; every profile stays within radial amplitude. A near-edge diagonal shuttle redirects inward instead of becoming motionless, while the other paths reflect at monitor edges. | A live negative-coordinate corner and Pause/Resume test passes; mixed-DPI, all-profile edge, and monitor-change behavior need further checks. |
| Settings and startup | Valid edits save immediately; non-finite values are rejected; motion settings update without restarting the delay; startup writes are checked and failed toggles revert; disabled repair remains disabled. A settings read error now stops launch visibly without changing startup registration. | Registry-denied error presentation and an interrupted settings write have not been fault-injected. |
| Lifecycle and resources | Lock/unlock notifications, sleep state, hook reset, a blocking hook message loop, and settings-window DPI resizing are in place; transient cursor and injection failures recover without ending the process. The settings window now uses a larger DPI-aware font and roomier controls. | Lock/unlock, sleep/wake, Explorer restart, reboot startup, live DPI changes, CPU use, and long-running resource stability need observation. |
| Installer | Clean install, upgrade, Start Menu entry, uninstall registration, startup cleanup, and settings retention were exercised. | An update while Wiggler itself is running has not been tested. |

The installer remains installed locally after validation, with Wiggler stopped and no temporary startup entry or test settings file left behind.

## Map of the codebase

| Area | Files | Current responsibility |
| --- | --- | --- |
| Pure behavior | `src/core.rs` | Settings bounds, runtime states, and four movement paths; eleven unit tests |
| Persistence | `src/settings.rs` | Local text settings with temporary-file replacement; four unit tests |
| Windows input | `src/native.rs` | Single-instance mutex, mouse hook, runtime loop, and `SendInput` |
| Windows shell | `src/ui.rs` | Tray, settings window, lifecycle messages, and settings edits |
| Startup | `src/startup.rs` | HKCU Run registration and repair |
| Entry and build | `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `build.rs`, `assets/app.manifest` | Process wiring, dependencies, icon and manifest |
| Installation | `installer/wiggler.iss` | Per-user installation, Start Menu shortcut, optional desktop shortcut |

The runtime has three threads: main runs the movement loop, one runs the mouse hook, and one runs the tray UI. `ControlState` shares settings and lifecycle flags between them. Each thread has an explicit startup handshake, and the hook event closes after its thread exits.

## Baseline findings, ordered by risk

The source references in this section point to the pre-change code and are preserved as the reasoning behind the fixes. Use the current status table above for the updated build.

### P0 — Correctness and release blockers

1. **Normal tray shutdown frees `UiContext` twice.** `run_ui` owns `context: Box<UiContext>` and also calls `drop(Box::from_raw(context_ptr))` without first transferring ownership via `Box::into_raw` (`src/ui.rs:107-169`). The original box is dropped again on return. This is undefined behavior on Exit. Keep a single owner and pass only a borrowed raw pointer to Win32; remove the second drop. Verify a normal Exit and repeated launch/exit cycles.

2. **Movement can continue with no working input monitor.** `NativeRuntime::start` spawns the hook thread and returns before hook installation succeeds (`src/native.rs:94-129`). Installation errors only print and retry, while `run` can begin injecting after the idle delay (`src/native.rs:139-185`). The UI thread also reports startup success before its tray exists (`src/ui.rs:65-75`). Require an explicit readiness result, fail closed when detection is unavailable, and make recovery state observable. Test a forced hook-install failure. Windows can [silently remove a timed-out low-level hook](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc), so reinstalling only on known errors and lifecycle messages does not establish ongoing health.

3. **A genuine event can arrive after the activity check and before injection.** The movement loop clears the activity flag, then reads the cursor, computes a point, and calls `SendInput` (`src/native.rs:168-184`). An event in that gap can be followed by at least one synthetic move. Design and measure a handoff where observed activity invalidates pending motion, then test activity injected at each handoff point. The observable guarantee should be phrased in terms of events the detector has received; cross-thread scheduling cannot prove zero elapsed time.

4. **Hook shutdown can use a closed event handle.** `NativeRuntime::drop` closes `activity_event` before joining the hook thread (`src/native.rs:190-199`), while the hook callback may still call `SetEvent` through the static `HOOK_SIGNAL` (`src/native.rs:234-244`). Stop and join/unhook the hook thread before closing its event, and make callback state lifetime explicit. Verify under repeated shutdown while mouse input is active.

5. **Uninstall can leave an enabled startup entry pointing to a removed executable.** The installer only installs and launches the binary (`installer/wiggler.iss`); it has no action that removes Wiggler's HKCU Run value (`src/startup.rs:8-43`). Add uninstall cleanup for the registration owned by this installation, preserving unrelated entries. Test install, enable startup, uninstall, then inspect the registry.

### P1 — Required product behavior

6. **Edits apply to memory immediately but persist only when the window closes.** `settings_proc` calls `update_settings(..., false)` for edit notifications and saves only on `WM_CLOSE` (`src/ui.rs:455-474`). A crash, logoff, or installer termination loses edited values. Persist each valid committed change promptly, with a short debounce for numeric typing if needed. Show or recover from write errors instead of discarding them.

7. **Startup configuration can silently disagree with the checkbox.** Every edit calls `startup::set_enabled` regardless of which setting changed, and its result is discarded (`src/ui.rs:387-423`). `set_enabled(true)` writes the Run value even if it already matches (`src/startup.rs:14-33`). An explicit "Repair Startup" action can enable startup even when the checkbox is off (`src/ui.rs:263-264`, `src/startup.rs:46-76`). Call registry updates only on startup changes, verify success before reporting enabled, and define repair as reconciliation with the saved choice. Test denied registry writes, disabled repair, and executable path changes.

8. **Any settings change restarts the inactivity countdown.** `Runtime::update_settings` always changes mode to `UserControl` and sets a fresh deadline (`src/core.rs:178-183`). Changing profile, amplitude, or speed while wiggling therefore stops movement for a full delay; changing startup also resets the timer. Apply path parameters immediately while retaining the current mode and resting point where sensible. Define how a delay edit recomputes the deadline from the last genuine event, then add state-transition tests.

9. **The movement profiles miss specified details.** Brownian's first sample is noise-offset rather than the resting point (`src/core.rs:113-122`), which can jump when motion starts. Lissajous has the 2:3 frequency ratio but no slow phase drift (`src/core.rs:109-112`). Diagonal can place the pointer about 1.41 times the configured amplitude from rest because both axes move by `A` (`src/core.rs:105-108`), contrary to the now-settled radial meaning of amplitude. Keep Brownian's initial position at rest, bound all profiles to the same radius, and add the specified phase evolution. Test continuity, radius, and long-run bounds with deterministic seeds.

10. **Screen boundaries are not constrained to usable monitors.** `inject_absolute` maps a generated point into the virtual desktop but never clips it to a monitor rectangle (`src/native.rs:277-300`). Near an edge the path can push into a boundary; the virtual desktop rectangle can include empty space between monitors. Compute the allowed path from the monitor containing the resting point, and refresh it after display changes. Test negative coordinates, mixed DPI, edges, and monitor removal.

11. **Lifecycle recovery coverage is incomplete.** The tray handles suspend, automatic resume, display/device/setting change, and Explorer's `TaskbarCreated` message (`src/ui.rs:190-224`), but does not explicitly handle lock/unlock or DPI change. The hook reset flag only causes uninstall/reinstall when the loop notices it; it cannot detect silent hook removal. Windows sends session lock/unlock through [`WM_WTSSESSION_CHANGE` after registration](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change). Define a lifecycle state table and exercise each PRD case on a real Windows session.

12. **The hook thread wakes roughly 1,000 times per second while idle.** Its `PeekMessageW` loop sleeps for 1 ms (`src/native.rs:219-228`). Use a blocking message loop with an explicit wakeup for reset and shutdown. Measure idle CPU and wakeups before and after. The [low-level hook documentation](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc) requires a message loop but does not require this polling pattern.

13. **Non-finite numeric values pass validation.** `Settings::validated` clamps ordinary values but does not reject `NaN` (`src/core.rs:44-59`). `read_f64` and the file parser accept parseable non-finite values (`src/ui.rs:765-766`, `src/settings.rs:117-128`). Reject all non-finite values at both UI and disk boundaries; test `NaN`, infinity, and malformed partial edits before they reach the position conversion.

### P2 — Usability, maintainability, and proof

14. **The settings window has weak error and creation checks.** It ignores the return from `CreateWindowExW` in `show_settings` (`src/ui.rs:367-384`), suppresses save and startup errors (`src/ui.rs:418-422`), and initializes controls from raw Win32 notifications. Add explicit creation and write outcomes and test the edit path. Keep the compact native UI; no larger UI framework is warranted.

15. **The release is not reproducibly verified from this checkout.** The ignored `target/release/wiggler.exe` and `dist/Wiggler-Setup-v1.0.0.exe` exist, but no build or acceptance instructions are tracked. `cargo` and `rustc` are not on the current PATH, so tests and a clean build could not be run during this review. The PRD requires a clean installer build and install test. Add a short release procedure or script with exact toolchain, Inno Setup command, checksums, clean install/uninstall, and actual Windows behavior results. Keep generated binaries ignored.

16. **The PRD was untracked at review time.** `Wiggler_v1.0.0_PRD.md` exists locally but was not in Git. The implementation's acceptance criteria can disappear from the next checkout. Commit the approved PRD or place an approved copy in a tracked product document.

## What is already sound

- `src/core.rs` has a small, independently testable runtime model; its existing tests cover default values, basic delay, pause, diagonal direction, and sampled bounds.
- Synthetic input is excluded using the Windows low-level injected flag (`src/native.rs:248-263`), the [documented flag for injected mouse events](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct).
- The single-instance mutex, per-user HKCU Run choice, tray restoration after Explorer restart, and per-user installer match the product's lightweight direction.
- Settings writes use a temporary file followed by replacement. The implementation needs error handling and persistence timing, not a new storage system.
- Dependencies are limited to `windows-sys` and the build-time Windows resource crate. No network dependency or telemetry appears in the reviewed source.

## Shortest path to full PRD acceptance

| Order | Work | Progress | Completion gate |
| --- | --- | --- | --- |
| 1 | Fix ownership and input-monitor readiness. | Implemented; installed normal exits pass. | Stress exit during physical input and force a hook-install failure. |
| 2 | Verify immediate physical override and synthetic isolation. | Generation recheck, injected flag, hook acknowledgement, and blocking pump implemented. | Real mouse, touchpad, click, and wheel tests show prompt override; injected movement never resets idle time. |
| 3 | Verify settings and startup failure handling. | Immediate persistence and startup toggle pass installed smoke tests. | Fault-inject a denied Run-key write and settings-file write; confirm the UI reports failure and retains accurate state. |
| 4 | Validate all profiles and monitor boundaries. | Pure radial and edge tests pass; a live diagonal test at a negative-coordinate corner passes. | Inspect the other three profiles at edges, mixed DPI, and monitor changes. |
| 5 | Exercise Windows lifecycle and installation. | Clean install, upgrade while stopped, normal exit, and uninstall pass. | Check lock/unlock, sleep/wake, Explorer restart, update while running, and login startup on the installed build. |
| 6 | Close the release gate. | Formatting, Clippy, 17 tests, optimized build, installer compilation, and visual inspection pass. | Record the remaining PRD checklist outcomes and a resource soak; fix any failures before calling v1.0.0 complete. |

Keep each step small enough to inspect. Avoid broad refactoring until the failure paths and acceptance tests identify a real seam. Prefer one explicit owner for each Win32 handle and a single source of truth for each state.

The remaining path with minimal hassle is to perform the device and Windows lifecycle checks above, then fix only failures those checks reveal. Avoid feature additions, module reshuffling, and extra decision rounds that do not close a PRD gap. The existing Rust and Inno Setup approach remains suitable.

## PRD traceability

"Present" means the source contains the intended mechanism. Installed behavior is stated separately; source presence alone does not prove a Windows lifecycle requirement.

| PRD sections | Requirement | Source assessment | Gap or acceptance evidence |
| --- | --- | --- | --- |
| 3–6 | Idle delay, genuine mouse override, synthetic isolation | Partial | Idle transition and live motion observed; low-level injected flag and hook acknowledgement are in source. Physical override timing remains unmeasured. |
| 7–9 | Smooth, bounded, non-drifting motion; four exact profiles | Partial | Radial unit tests pass and diagonal motion was observed; visual behavior of the other three profiles is unverified. |
| 10 | Multi-monitor, mixed DPI, screen-edge behavior | Partial | Virtual coordinates and edge adaptation are in source; a negative-coordinate corner test passed. Mixed-DPI and topology-change tests remain. |
| 11–13 | Simple controls, immediate apply, no save workflow | Present and partly tested | A delay edit persisted before window close; motion-setting state tests pass. |
| 14–16 | Compact native settings and tray-first operation | Present and tested | Installed tray and settings window opened; full DPI-aware screenshot showed all controls and units. Focus behavior remains unmeasured. |
| 17 | Pause and resume | Present and tested | Core test and installed tray Pause/Resume test pass, including at a screen edge. |
| 18–20 | Reliable startup, repair, single instance | Partial | Startup toggle, disabled repair, single instance, and uninstall cleanup pass; reboot/login and registry-denied cases remain. |
| 21–23 | Windows lifecycle, recovery, long-running stability | Partial | Lock/unlock handlers, hook acknowledgement, and blocking pump exist; lifecycle and soak tests remain. |
| 24–28 | Local-only operation and small codebase | Present in source | No network or telemetry paths found; no broad rearchitecture was added. |
| 29–30 | Installed release and comprehensive validation | Partial | Installer compiled; clean install, update while stopped, launch, Start Menu, and uninstall passed. Full PRD checklist remains open. |

The release gate is the PRD's definition of done. The installed smoke tests establish basic behavior, while the remaining device and lifecycle checks determine whether the result is ready to call v1.0.0 complete.

## Validation matrix

| Scenario | Current evidence | Needed evidence |
| --- | --- | --- |
| Idle delay, pause, simple bounds | Eleven core tests, two native edge tests, and live motion plus Pause/Resume | Other profile visual checks |
| Persistent settings | Four file tests and installed immediate-edit smoke test | Denied write and interrupted write |
| Input isolation and override | Injected-flag source check, acknowledgement recovery, and live motion | Real mouse, touchpad, wheel, and handoff timing |
| Recovery | Lock/sleep handlers and transient native-call retry | Lock/unlock, sleep/wake, hook loss, Explorer restart, display changes |
| Installation | Clean install, stopped upgrade, startup toggle, uninstall, Start Menu | Update while running and reboot/login startup |
| Long-running behavior | Blocking hook loop and short live run | Resource measurements and multi-day or accelerated soak |

## Verification record

The Windows build used Rust/Cargo 1.98.1, Visual Studio Build Tools 2022 with the MSVC C++ workload, and Inno Setup 6.7.3. The executable was built in the MSVC developer environment. On 2026-09-28:

- `cargo fmt --all` completed, and `cargo clippy --all-targets --locked -- -D warnings` passed.
- `cargo test --locked` passed all 17 tests: 11 core, four settings, and two Windows monitor-edge tests.
- `cargo build --release --locked` and `ISCC installer\wiggler.iss` succeeded.
- A clean silent install created the executable, Start Menu shortcut, and Windows uninstall entry. An upgrade while the app was stopped succeeded.
- The installed app created its tray window, rejected a second instance, saved a delay edit before settings closed, applied and removed Start with Windows, kept Repair Startup disabled when startup was off, kept running after settings closed, and exited normally.
- With a one-second delay and five-pixel amplitude, the installed app produced multiple cursor positions and exited normally while motion was active. A follow-up test at the top-left corner of a monitor with negative Y coordinates found and then verified the fix for an edge trap; tray Pause/Resume also passed there.
- A later live linear-profile run was inconclusive: the cursor moved hundreds of pixels in both X and Y before sampling, whereas Linear can only change X by at most five pixels. The run cannot establish linear behavior or a product defect; its result is excluded from the pass count.
- Silent uninstall removed the executable, Start Menu shortcut, and matching HKCU Run entry, while retaining a settings file. The test settings file and Run entry were then cleaned up. The final build was reinstalled and is currently stopped.
- The installed settings window was captured at physical DPI size and visually inspected: profile, delay, amplitude, speed, units, startup checkbox, and native close button were visible without clipping.

After the installed smoke run, a small pass fixed settings-read failure handling and added a `WM_DPICHANGED` settings-window resize. Following user feedback that the font was too small, the settings controls now use a 20-pixel-at-96-DPI Segoe UI font, scaled with window DPI. The window, labels, fields, and buttons have more room. The rebuilt app was installed, and a read-only screenshot of the actual settings window was inspected: text and units are legible with no clipping. The existing user settings file was preserved. Formatting, strict Clippy, all 17 tests, the optimized build, and installer compilation passed again. The rebuilt installer is `dist/Wiggler-Setup-v1.0.0.exe` (SHA-256 `E3C3D840D37BBE46BE09BDEC28AD08D36E085EB2772820CC534C2DF730F7CB42`). The release executable is `target/release/wiggler.exe` (SHA-256 `8D57383B8C26E5E7CF5F972ECE7A8EACE92FE616E898C6319C5B19DA67448B5F`). Generated binaries remain ignored by Git. Rust, MSVC Build Tools, and Inno Setup were installed on this machine for verification.

To reproduce the build, open an MSVC x64 developer shell in this checkout and run `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked`, then compile `installer\wiggler.iss` with Inno Setup 6 (`ISCC.exe`). Install the resulting setup executable for the remaining physical-device and Windows lifecycle acceptance checks.

The GitHub Actions workflow in `.github/workflows/release.yml` runs those checks on a Windows runner, builds the installer, and publishes it from a `v*` tag. Tags with a suffix such as `v1.0.0-rc.1` produce a prerelease while PRD acceptance checks are open. The workflow has been prepared locally; a GitHub run is needed to verify the hosted runner and publication path.

Windows-specific facts used in this review come from [Microsoft's low-level mouse hook documentation](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc), [`MSLLHOOKSTRUCT`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct), [`SendInput`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput), [Taskbar behavior](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar), and [session change messages](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change). `SendInput` can be limited by UIPI; failures need to be handled as real failures rather than assumed recoverable in place.

## Settled product decisions

- A stationary mouse click is genuine mouse activity and resets the inactivity delay.
- Amplitude is a radial maximum from the resting point for every profile.
- If enabling Start with Windows fails, the checkbox reverts and shows a brief error.
- Uninstall removes the startup registration but retains the settings file.

The requested improvement target is Wiggler's conformance to the existing PRD as soon as practical and without unnecessary hassle. The PRD already decides that genuine input must override motion immediately, so acceptance tests should measure latency and reject any injection after observed activity. Input technology should be selected after device-coverage evidence, not by asking for a framework preference. No ADR is needed for these straightforward product choices.
