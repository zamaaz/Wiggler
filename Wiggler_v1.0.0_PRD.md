# Wiggler — The Fable v1.0.0
## Final Product Requirements & Master Implementation Specification

> **One version. One purpose. One standard.**
>
> v1.0.0 is not a milestone on the way to something else. It is the finished product.
>
> There is no v1.1, no v2, no roadmap, no promise of future features, and no assumption that this application will need continued development.
>
> Build it so completely that the best outcome is that nobody ever needs to touch the code again.

---

# 0. The Fable

Wiggler is a tiny Windows utility whose job is extraordinarily simple:

> When the user has not moved the mouse for a configured amount of time, Wiggler moves the cursor subtly and continuously. The instant the user returns to the mouse, Wiggler disappears from control and lets the user work normally.

That is the entire product.

The sophistication is not supposed to be visible.

The application should be so reliable and unobtrusive that, after initial configuration, the user forgets it exists.

The guiding philosophy is:

> **It should cease to exist in the background.**

While the user is using the mouse (even if the user is scrolling through trackpad or wheel):

> Wiggler vanishes into thin air.

When the user stops using it:

> Wiggler quietly comes out of thin air, performs its designated motion, and waits.

When the user touches the mouse again:

> Wiggler vanishes again, immediately.

This behavior is the product.

Everything else exists to support it.

---

# 1. What We Learned

This specification incorporates lessons learned from earlier prototypes without requiring the implementation agent to know, inspect, preserve, or understand their source code.

Treat these as **product and engineering lessons**, not as instructions to reproduce the old architecture.

### 1.1 Reliability matters more than features

A feature-rich jiggler that silently stops working is worse than a tiny jiggler that does one thing correctly for years.

Reliability takes precedence over:

- visual novelty
- architectural complexity
- feature count
- mathematical cleverness
- framework preference
- abstractions
- UI decoration

### 1.2 The UI is not the product

The settings window will probably be used for a few seconds during initial configuration and then forgotten.

The UI therefore does not need to be impressive.

It needs to be:

- clear
- compact
- predictable
- fast
- boring
- powerful

**Boring is intentional.**

### 1.3 Overengineering is a failure mode

Earlier attempts demonstrated that this tiny utility can become unnecessarily large when an implementation agent interprets "robust" as "enterprise architecture."

Do not create architecture for architecture's sake.

Do not create dozens of source files merely to make the project look organized.

Do not create layers that exist only to forward calls to another layer.

Do not add abstractions until a real requirement demands them.

A small application should have a small codebase.

### 1.4 The sophisticated part belongs underneath the surface

The application may need careful handling of Windows input hooks, session changes, sleep/wake, startup, recovery, packaging, and persistence.

That complexity is acceptable **only where it directly buys reliability**.

The user should never see that complexity.

---

# 2. Definition of Done

Fable v1.0.0 is complete when all of the following are true:

- It behaves correctly during normal use.
- Genuine mouse/trackpad activity immediately overrides synthetic movement.
- Synthetic movement never counts as user activity.
- Movement is smooth, bounded, and non-drifting.
- All four movement profiles behave as specified.
- Configuration changes apply immediately.
- The application runs quietly in the tray.
- The settings window is compact and deliberately simple.
- It starts reliably with Windows.
- It survives normal Windows lifecycle events.
- It can recover from relevant internal failures.
- It remains stable during very long-running sessions.
- It does not require unnecessary permissions.
- It consumes negligible resources.
- It has no unnecessary network dependency or telemetry.
- The release artifact is a directly usable Windows executable - not portable but installable with proper Start Menu entries.
- Installation/distribution is simple.
- The final codebase is small enough to understand without archaeology.

There is no "future phase" hiding behind these requirements.

**This is v1.0.0.**

---

# 3. Core User Behavior

The primary state flow is:

```text
                 Mouse activity
                       │
                       ▼
              ┌─────────────────┐
              │   USER CONTROL  │
              │   Wiggler OFF   │
              └────────┬────────┘
                       │
                       │ no mouse activity
                       │ for N seconds
                       ▼
              ┌─────────────────┐
              │    WIGGLING     │
              │ continuous path │
              └────────┬────────┘
                       │
                       │ genuine mouse activity
                       ▼
              ┌─────────────────┐
              │   USER CONTROL  │
              │   Wiggler OFF   │
              └─────────────────┘
```

Default inactivity delay:

**5 seconds**

The user may configure the delay.

When the delay expires, Wiggler begins the selected movement profile.

When genuine mouse activity is detected:

1. Stop synthetic movement immediately.
2. Give the cursor fully back to the user.
3. Reset the inactivity countdown.
4. Remain completely inactive until the delay expires again.

---

# 4. Activity Definition

Only mouse/pointer activity matters.

### Activity that must count

- Physical mouse movement
- Touchpad/trackpad movement
- Genuine pointer movement exposed by Windows

### Activity that must NOT count

- Keyboard presses
- Keyboard releases
- Typing
- Application switching
- Window focus changes
- CPU activity
- Network activity
- Audio
- Notifications
- Arbitrary application events

Wiggler is a **mouse jiggler**, not a general-purpose activity detector.

---

# 5. Immediate User Override

This is one of the most important behavioral requirements.

The instant the user moves the physical mouse or trackpad while Wiggler is generating movement:

> **Wiggler must stop.**

Do not wait for:

- a movement cycle
- a path segment
- a timer interval
- the end of an animation
- a debounce window
- a return-to-origin phase

The user's input must win immediately.

The desired perception is:

> "I touched the mouse and Wiggler disappeared."

Not:

> "I touched the mouse and it fought me for another 300 ms."

---

# 6. Synthetic Input Isolation

Wiggler's own generated mouse movement must never be mistaken for genuine user activity.

This is a hard correctness requirement.

Avoid solutions based purely on timing guesses or arbitrary delays.

Use the appropriate Windows-native mechanism to distinguish injected/synthetic input from genuine physical input.

The implementation must prevent:

```text
Wiggler moves cursor
      ↓
Input monitor sees movement
      ↓
Wiggler thinks user moved mouse
      ↓
Wiggler stops
      ↓
Timer expires
      ↓
Wiggler starts again
      ↓
repeat forever
```

The movement engine and activity detector must have a reliable relationship with injected input.

---

# 7. Movement Philosophy

Movement should feel intentional, subtle, and continuous.

It must:

- be bounded
- be smooth
- avoid positional drift
- remain local to the user's cursor position
- stop immediately on user input
- resume around the cursor's current location after inactivity

It must not:

- wander across the screen
- seek screen corners
- become trapped at an edge
- continuously drift
- randomly teleport
- fight the user
- create noticeable cursor seizure
- require a discrete "move every X seconds" cycle

The default motion is continuous.

---

# 8. Movement Profiles

Fable v1.0.0 has exactly four primary movement profiles.

These are not placeholders for a future system. They are the finished profile set.

## 8.1 Linear

**Definition:**

Bounded oscillation along a single horizontal axis.

Conceptually:

```text
rest ←────────→ rest
```

The cursor smoothly traverses the horizontal displacement and returns.

Properties:

- single axis
- continuous
- bounded
- deterministic
- smooth
- no drift

This is the simplest profile.

---

## 8.2 Diagonal

**Default profile.**

**Definition:**

A diagonal shuttle between the resting point and an upper-left diagonal point.

Conceptually:

```text
      ↖
     ↖
    ↖
   ↖
  ●
```

The motion is:

```text
rest → upper-left diagonal corner → rest → repeat
```

Critical constraint:

> **It must NOT cross through the resting point into the opposite/down-right direction as an oscillating diagonal.**

The intended behavior is a shuttle that retraces its path:

```text
rest
  ↖
    ↖
      ↖
        ↖
          corner
        ↘
      ↘
    ↘
  ↘
rest
```

This is the primary profile because it most closely matches the intended subtle mouse-jiggling behavior.

---

## 8.3 Lissajous

**Definition:**

A smooth Lissajous path using a 2:3 frequency relationship with slow phase drift.

It should feel organic rather than mechanically repetitive.

Requirements:

- smooth continuous movement
- bounded to the configured amplitude region
- no abrupt direction changes
- no positional drift
- slow phase evolution
- visually subtle

The exact mathematical implementation is an engineering choice.

The user should only experience:

> a smooth, natural-looking bounded motion.

Do not expose mathematical terminology in the normal UI.

---

## 8.4 Brownian

**Definition:**

A seeded value-noise-based wander that remains strictly inside the configured amplitude radius.

The goal is:

> random-looking, not randomly jumping.

Requirements:

- seeded/deterministic noise
- smooth interpolation
- bounded radius
- no sudden teleportation
- no edge seeking
- no accumulated drift
- continuous motion
- immediate interruption on user input

A noise function such as Perlin/value noise may be used if appropriate, but the exact algorithm is an implementation detail.

The important requirement is the resulting behavior.

---

# 9. Movement Bounds

Every profile must remain inside its configured movement boundary.

If amplitude is `A`, the generated position must never exceed the intended bound.

The movement engine must not accumulate deltas indefinitely.

Prefer absolute/path-based positioning around a reference point rather than repeatedly adding movement deltas in a way that can accumulate floating-point or integer drift.

When jiggling begins:

1. Capture the current cursor location as the reference.
2. Generate the selected path around that reference.
3. Keep the entire path bounded.
4. When genuine user input arrives, abandon the old reference.
5. When jiggling resumes later, establish a fresh reference from the user's current cursor.

---

# 10. Screen and Multi-Monitor Behavior

Do not assume:

- one monitor
- one resolution
- one DPI scale
- positive-only coordinates
- a fixed desktop size

Support Windows' virtual desktop coordinate system.

The movement must behave correctly across:

- multiple monitors
- different resolutions
- mixed DPI/scaling
- negative virtual coordinates
- monitors being attached/detached

Do not allow a path to push the cursor into an unusable screen boundary.

If the cursor is near a boundary, safely constrain the movement while preserving the profile as much as practical.

---

# 11. Timing

There is **no required movement interval** in the primary design.

The jiggling is continuous.

There are two conceptually different timings:

### Inactivity delay

How long genuine mouse inactivity must persist before Wiggler starts.

Default:

**5 seconds**

### Movement speed

How quickly a profile traverses its continuous path.

These must not be conflated.

An optional interval-based mode is not required for v1.0.0.

Do not build the primary engine around periodic "move once every N seconds" behavior.

---

# 12. Configuration UI

The settings UI is intentionally simple.

The visual reference is the philosophy of the original Wiggler application:

> dense, direct, boring, useful.

It does not need to impress the user.

It needs to get out of the way.

## Required controls

The settings window contains only the controls needed to configure Wiggler.

### 12.1 Title

A simple title:

**Wiggler**

Optionally a small subtitle if it genuinely improves clarity, but do not spend half the window on branding.

### 12.2 Movement profile

A **dropdown/select control**.

Options:

```text
Linear
Diagonal
Lissajous
Brownian
```

Default:

**Diagonal**

Do NOT use radio buttons.

### 12.3 Start moving after

A numeric input.

Default:

```text
5 seconds
```

This is a plain numeric field/spin control, not a slider.

### 12.4 Amplitude

A numeric input.

Example:

```text
5 pixels
```

No slider.

### 12.5 Speed

A numeric input.

No slider.

The exact numeric scale is an implementation decision, but it must be understandable and bounded.

### 12.6 Start with Windows

A standard checkbox.

Changing it applies immediately.

### 12.7 Close

The settings window has a normal close button.

Closing the settings window must **not** stop Wiggler.

The process continues in the background/tray.

---

# 13. Immediate Apply

There is **no Save button**.

There is:

- no Save
- no Apply
- no OK
- no Cancel workflow

Every configuration change applies immediately.

Examples:

```text
User changes profile
        ↓
New profile becomes active immediately.

User changes inactivity delay
        ↓
New delay is used immediately.

User changes amplitude
        ↓
New amplitude is used immediately.

User changes speed
        ↓
New speed is used immediately.

User toggles Start with Windows
        ↓
Startup configuration changes immediately.
```

Do not create a fake save workflow for settings that are simple enough to apply directly.

---

# 14. UI Visual Language

The window should be:

- small
- rectangular
- compact
- rounded window corners - subtle
- clean
- high information density
- easy to understand
- deliberately unremarkable
- the close button should be the windows default button and not ugly self created one.

A subtle modern Windows appearance is welcome, but **do not turn this into a Fluent design showcase**.

The intended aesthetic is:

> **Boring but powerful.**

Avoid:

- giant cards
- oversized typography
- huge empty spaces
- dashboards
- status dashboards
- elaborate navigation
- radio-button groups
- sliders
- decorative illustrations
- unnecessary icons
- excessive animation
- glassmorphism for its own sake
- rounded-card-everything design
- web-app aesthetics
- neon accents
- "premium" visual theater

Do not make the window visually louder than the function.

---

# 15. UI Layout

A compact conceptual layout:

```text
┌─────────────────────────────────────┐
│ Wiggler                         ✕  │
├─────────────────────────────────────┤
│                                     │
│ Movement profile                    │
│ [ Diagonal                    ▼ ]  │
│                                     │
│ Start moving after                 │
│ [ 5 ] seconds                      │
│                                     │
│ Amplitude                           │
│ [ 5 ] pixels                       │
│                                     │
│ Speed                               │
│ [ 3 ]                              │
│                                     │
│ ☑ Start with Windows                │
│                                     │
└─────────────────────────────────────┘
```

This is conceptual, not a pixel-perfect requirement.

The agent should optimize final spacing and control sizing while preserving the philosophy.

---

# 16. Tray-First Behavior

Wiggler is primarily a background/tray application.

The settings window is secondary.

After configuration:

- close the settings window
- leave the tray process running
- do not leave a taskbar application unnecessarily visible
- do not steal focus

The tray menu should be compact and predictable.

Suggested actions:

```text
Wiggler
──────────────
Status
Pause / Resume
Settings
Start with Windows
Repair Startup
──────────────
Exit
```

The exact native presentation is an implementation detail.

Do not make the tray menu a miniature dashboard.

---

# 17. Pause

A manual Pause action may be provided from the tray.

When paused:

- synthetic movement stops
- inactivity does not automatically re-enable movement
- the process remains alive
- the user can Resume from the tray

When resumed:

- reset the inactivity timer
- wait for the configured delay
- resume normal behavior

---

# 18. Startup Reliability

Startup reliability is one of the highest-priority requirements.

The user should be able to enable "Start with Windows" and forget about it.

It must behave correctly after:

- reboot
- shutdown/startup
- logout/login
- normal Windows updates
- application replacement/update
- Explorer restart

Choose an appropriate Windows-native startup mechanism.

Possible approaches include:

- Task Scheduler
- HKCU Run
- another suitable user-level startup mechanism

Do not blindly combine multiple mechanisms merely to appear more reliable.

Use the smallest mechanism or combination that is actually robust.

---

# 19. Self-Healing Startup

If the application can determine that its startup registration has become invalid, it should be able to repair it quietly.

A reasonable design is:

1. On launch, verify startup configuration when enabled.
2. Compare the registered executable path/command with the currently running executable.
3. If repair is necessary, repair it without interrupting the user.
4. Avoid unnecessary repeated writes.
5. Avoid creating duplicate startup entries/tasks.

The application should not spawn a pile of duplicate scheduled tasks or registry values.

---

# 20. Single Instance

Only one Wiggler process should operate at a time.

If the user launches another copy:

```text
Existing instance → remains active
New instance → exits gracefully
```

Do not allow multiple input hooks or movement engines to compete for the cursor.

---

# 21. Windows Lifecycle Reliability

The application must remain functional across common Windows lifecycle events.

Test and handle:

- lock
- unlock
- sleep
- wake
- display sleep
- monitor changes
- DPI changes
- Explorer restart
- user logon/logoff
- multiple-monitor changes

If a native input hook or internal subsystem becomes invalid after a lifecycle event, recover it.

Prefer local subsystem recovery over restarting the entire process.

---

# 22. Recovery

The application must distinguish:

> process is running

from:

> application is actually functional.

Relevant subsystems may include:

- mouse activity detection
- synthetic movement engine
- timers
- lifecycle notifications
- startup state

If a recoverable subsystem fails:

1. Detect the failure.
2. Attempt a safe recovery.
3. Verify recovery.
4. Continue operation.

Do not spam the user with technical error messages for recoverable failures.

---

# 23. Long-Running Stability

The intended runtime is not "a few minutes."

The application may run continuously for days, weeks, or longer.

Therefore:

- no busy waiting
- no memory leaks
- no unbounded queues
- no runaway timers
- no uncontrolled thread creation
- no excessive polling
- no excessive CPU usage
- no resource accumulation
- safe cleanup of native handles/hooks
- deterministic shutdown

Resource usage should be effectively negligible.

---

# 24. Privacy and Network Behavior

Wiggler does not need the internet.

Prefer:

- no network dependency
- no telemetry
- no analytics
- no cloud
- no accounts
- no remote service
- no external control

The application should operate locally.

Request only the permissions genuinely required.

---

# 25. Engineering Simplicity

This is a tiny utility.

Treat smallness as a product feature.

### Do not optimize for file count by itself.

Instead, optimize for **essential moving parts**.

A project with 8 well-justified files is better than a project with 3 giant god-files.

A project with 3 files is better than one with 30 if the code remains clear.

### Do not create:

- dependency injection frameworks
- service locators
- factories for trivial constructors
- repositories
- provider layers
- event-bus abstractions unless genuinely required
- excessive interfaces/traits
- unnecessary background services
- unnecessary async runtimes
- unnecessary third-party dependencies
- configuration frameworks for a handful of settings
- artificial "manager" classes that only forward calls

### Rule

> **Every abstraction must earn its existence.**

If removing an abstraction makes the program easier to understand without harming reliability, remove it.

---

# 26. Code Organization

Organize code around real responsibilities rather than arbitrary architectural fashion.

At minimum, the logical responsibilities are:

```text
Application lifecycle
Mouse activity detection
Movement/path generation
State/timing
Settings persistence
Windows startup
Tray/settings UI
```

These do not necessarily require seven separate files.

The implementation agent should choose the smallest coherent structure.

The code should be understandable top-to-bottom by a competent developer.

Avoid:

- 1,000-line god modules
- enormous `app` files
- duplicated state logic
- duplicated Windows API wrappers
- excessive plumbing
- "framework inside the application"

---

# 27. Technology

The implementation technology is an engineering decision.

Prefer:

- native Windows behavior
- small runtime footprint
- direct access to Windows input/lifecycle APIs
- simple deployment
- long-running reliability

Rust is a strong candidate for the core because the application is small, native, background-oriented, and long-running.

However:

> **Do not choose a technology merely because it sounds sophisticated.**

Do not force a particular UI toolkit or architecture if it makes the product larger, more fragile, or harder to maintain.

For the UI, optimize for:

> **native-feeling, lightweight, simple, and visually clean.**

Not:

> maximum framework purity.

---

# 28. Dependencies

Minimize dependencies.

Every external dependency must answer:

> What concrete problem does this solve?

Prefer standard libraries and native Windows functionality where practical.

Do not add a crate/library merely because it provides a trendy abstraction over a simple operation.

Dependency count and binary size are not goals in themselves; **unnecessary dependency surface is**.

---

# 29. Installation & Release Artifact

Wiggler is an **installed Windows application**, not a portable utility.

The end user should install it normally, after which the application should live in the expected Windows installation location and be discoverable from the **Start Menu**.

The user should **not** have to remember where an `.exe` was downloaded or manually keep track of a portable binary.

## Required

- Release build
- 64-bit Windows target unless there is a compelling reason otherwise
- A proper Windows installer is required
- The installer must install Wiggler to an appropriate per-user or system application location
- The installer must create a Start Menu entry
- The installed application must be launchable from the Start Menu
- The installer should create an uninstaller / register the application correctly with Windows
- Installation should be straightforward and require no developer tooling
- The installed application should run normally without requiring the user to keep the installer or package around
- The installer should configure the application's normal installation state; runtime "Start with Windows" remains a user-configurable application setting
- The application should remain available from the Start Menu even though its normal runtime presence is only in the system tray/background
- The first-run experience should be quiet and minimal

## Installer UX

The desired flow is:

```text
Run installer
    ↓
Install Wiggler
    ↓
Wiggler is installed normally
    ↓
Start Menu entry exists
    ↓
Wiggler launches / enters the tray
    ↓
The installer is no longer relevant
```

After installation, the user should not need to hunt for `Wiggler.exe`.

## Packaging

An MSI is preferred if it provides a clean, conventional Windows installation experience. A well-implemented alternative Windows installer format is acceptable if it integrates properly with Windows.

The final release may contain a standalone executable internally as an implementation/build artifact, but the **user-facing distribution must be the installer**, not a portable `.exe`.

The implementation agent must actually build and verify the installer and perform a clean install test.

The agent must not stop at source code or at an uninstalled executable.

---

# 30. Validation Requirements

Before declaring v1.0.0 finished, test the actual built application.

## Behavior

- [ ] Launches successfully.
- [ ] Appears in the tray.
- [ ] Does not unnecessarily remain in the taskbar.
- [ ] Does not steal focus.
- [ ] Waits for the configured inactivity period.
- [ ] Starts continuous movement.
- [ ] User mouse movement immediately stops synthetic motion.
- [ ] Timer resets immediately.
- [ ] Movement resumes after the configured idle period.
- [ ] Keyboard input does not interrupt the jiggler.
- [ ] Synthetic movement does not trigger the activity detector.

## Profiles

- [ ] Linear is bounded and smooth.
- [ ] Diagonal is a rest ↔ upper-left shuttle and never crosses into a down-right excursion.
- [ ] Lissajous is smooth, bounded, and uses the intended 2:3 relationship with slow phase drift.
- [ ] Brownian is smooth, seeded, bounded, and does not teleport.

## Configuration

- [ ] Profile changes apply immediately.
- [ ] Inactivity delay changes apply immediately.
- [ ] Amplitude changes apply immediately.
- [ ] Speed changes apply immediately.
- [ ] Start-with-Windows changes apply immediately.
- [ ] No Save/Apply button is required.
- [ ] Closing the settings window leaves the tray process running.

## Reliability

- [ ] Survives reboot.
- [ ] Survives logout/login.
- [ ] Survives lock/unlock.
- [ ] Survives sleep/wake.
- [ ] Survives display/monitor changes.
- [ ] Works with multiple monitors.
- [ ] Works with mixed DPI.
- [ ] Works near screen boundaries.
- [ ] Recovers from relevant hook/subsystem failures.
- [ ] Does not accumulate resource usage over long sessions.

## Process integrity

- [ ] Second instance exits cleanly.
- [ ] No duplicate startup registrations are created.
- [ ] No duplicate tray instances are created.
- [ ] Native handles/hooks are cleaned up.
- [ ] Application exits cleanly.

## Packaging

- [ ] Release build succeeds from a clean environment.
- [ ] Final `.exe` launches independently.
- [ ] Required runtime dependencies are present or clearly accounted for.
- [ ] No development environment is needed by the end user.

---

# 31. UI Quality Test

Before shipping, visually inspect the settings window and ask:

> Does this look like a small Windows utility that I configure once and forget?

If it instead looks like:

- a dashboard
- a SaaS product
- a design-system showcase
- a game launcher
- a developer tool
- a concept-art mockup

then the UI is wrong.

The correct visual result is intentionally plain.

It should communicate:

> **"Here are the settings. Change them. Close the window. Done."**

---

# 32. Anti-Overengineering Test

Before shipping, inspect the codebase.

Ask:

- Can a developer understand the entire project without a diagram?
- Does every major file exist for a real reason?
- Are there abstractions that merely forward function calls?
- Are there background threads that could be removed?
- Are there dependencies that could be removed?
- Are there duplicate state representations?
- Is there a god module?
- Is configuration more complicated than the settings UI?
- Is the architecture solving hypothetical future problems?

If yes, simplify it.

The codebase should be small enough that its complexity can be held in one person's head.

---

# 33. What Not To Build

Do not add:

- Accounts
- Cloud sync
- Telemetry
- Analytics
- Custom scripting
- Plugins
- Extensions
- Web dashboards
- Remote control
- AI features
- Notifications unrelated to configuration/failure
- Feature marketplaces
- Update systems
- Subscription systems
- In-app purchases
- "Pro" tiers
- Gamification
- Usage statistics
- Fancy animations
- A large onboarding flow
- A roadmap

Fable v1.0.0 is the finished product.

---

# 34. The One-Version Principle

Do not design v1.0.0 around future versions.

Do not leave:

```text
TODO: v2
TODO: future profiles
TODO: future architecture
TODO: plugin API
TODO: extension system
```

unless a TODO is required to finish v1.0.0.

There is no promised v2.

There is no feature backlog hidden inside the product requirements.

Build the complete thing now.

---

# 35. The Ultimate Product Test

Imagine a user installs Wiggler in 2026.

They configure:

```text
Profile: Diagonal
Start moving after: 5 seconds
Amplitude: small
Speed: subtle
Start with Windows: enabled
```

They close the settings window.

Months pass.

The computer reboots.

The user logs in.

Wiggler starts silently.

The user never sees it.

They stop touching the mouse.

Five seconds later, the cursor begins its tiny diagonal motion.

They touch the mouse.

Wiggler instantly disappears from control.

They work.

They stop touching it again.

Wiggler quietly returns.

No prompts.

No distraction.

No maintenance.

No thinking.

That is the product.

---

# 36. Final Directive to the Implementation Agent

Build **Fable v1.0.0** as a finished, self-contained Windows utility.

You are not inheriting an old codebase.

You are not required to reproduce the architecture of any prototype.

You are inheriting only the lessons learned:

- Reliability matters more than complexity.
- The app must disappear when the user uses the mouse.
- Only genuine mouse activity matters.
- Synthetic input must never count as activity.
- Movement must be smooth, bounded, subtle, and non-drifting.
- Startup must be reliable.
- Recovery must be quiet.
- The settings window must be deliberately boring.
- Configuration must apply instantly.
- The codebase should be as small as the problem permits.
- The final deliverable must be a real working Windows executable.

Use your judgment for implementation details.

Do research where Windows behavior is subtle.

Prefer deterministic, boring solutions over clever ones.

Do not over-engineer.

Do not make the UI more complicated than the product.

Do not make the architecture more complicated than the reliability requirements.

Do not leave the user with source code and instructions to finish the job.

Build it.

Test it.

Package it.

Verify it.

Then stop.

---

# FABLE v1.0.0

**No sequel.**

**No roadmap.**

**No "coming soon."**

**Just the finished utility.**

> **Boring UI. Invisible runtime. Brutal reliability.**
>
> **One version. One job. Done.**
