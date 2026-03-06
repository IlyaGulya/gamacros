# FSM/HFSM Refactor Plan

## Why Change the Architecture

The project is already split into sensible crates, but the runtime behavior in `gamacrosd` is still coordinated through a large mutable orchestration layer with state spread across:

- `crates/gamacrosd/src/main.rs`
- `crates/gamacrosd/src/app/gamacros.rs`
- `crates/gamacrosd/src/app/stick/repeat.rs`
- `crates/gamacrosd/src/app/stick/tick.rs`

This works today, but it makes the following harder than necessary:

- reasoning about current runtime mode
- testing transitions without real side effects
- adding richer controller/app modes
- evolving toward explicit FSM/HFSM behavior

The goal is not to force a state machine framework into the codebase. The goal is to make state, events, transitions, and side effects explicit enough that FSM/HFSM becomes a natural shape of the runtime.

## Target Outcomes

After the refactor, the runtime should have:

1. a single domain event model
2. a centralized runtime state model
3. pure transition logic separated from side effects
4. named timer events instead of scattered scheduling flags
5. clear boundaries between top-level runtime state and per-controller state

## Architectural Direction

### 1. Introduce a Unified Event Layer

All external inputs should be normalized into one domain event type.

Example shape:

```rust
enum DomainEvent {
    Controller(ControllerEvent),
    Activity(ActivityEvent),
    Profile(ProfileEvent),
    Api(ApiCommand),
    Timer(TimerEvent),
    System(SystemEvent),
}
```

This becomes the only input to the runtime core.

### 2. Centralize Runtime State

Instead of splitting operational state between `main.rs`, `Gamacros`, and stick scheduler internals, move toward one explicit runtime state tree.

Example shape:

```rust
struct RuntimeState {
    mode: RuntimeMode,
    profile: ProfileState,
    app: AppState,
    binding: BindingContext,
    controllers: AHashMap<ControllerId, ControllerState>,
}
```

### 3. Separate Transition Logic from Effects

The runtime core should decide what must happen, but not perform the action directly.

Target split:

- reducer: `State + Event -> Effects`
- effect runner: executes keyboard, rumble, shell, and timer operations

Example shape:

```rust
fn reduce(state: &mut RuntimeState, event: DomainEvent) -> Vec<Effect>
```

### 4. Move Toward Hierarchical State

The future state model should have at least two levels:

- top-level runtime mode
- per-controller state

Potential top-level states:

- `Booting`
- `AwaitingProfile`
- `Active`
- `Suspended`
- `ProfileError`
- `ShuttingDown`

Potential per-controller states:

- `Disconnected`
- `Idle`
- `ButtonsActive`
- `StickActive`
- `Repeating`

Stick behavior can later become a nested substate inside each controller.

## Refactor Phases

## Phase 1: Introduce `DomainEvent`

### Goal

Stop letting `main.rs` contain hidden domain rules.

### Actions

- add a `DomainEvent` enum in a new domain-oriented module
- convert controller input, activity changes, profile updates, API commands, and wakeups into `DomainEvent`
- make the main loop a thin dispatcher instead of a decision-heavy orchestration layer
- keep behavior unchanged during this phase

### Definition of Done

- `main.rs` primarily receives inputs and forwards `DomainEvent`
- no new runtime rules are added directly to `main.rs`
- current behavior remains intact

### Notes

This is the safest first step because it improves structure without changing semantics.

## Phase 2: Introduce `BindingContext`

### Goal

Separate profile storage from effective runtime bindings.

### Actions

- add a `BindingContext` that contains resolved runtime rules for the current app/profile state
- rebuild it only when profile or active app changes
- move blacklist, fallback-to-common, shell selection, and effective stick/button rule resolution into this layer
- stop reading raw profile maps directly from button and stick handlers where possible

### Definition of Done

- button/stick evaluation reads from `BindingContext`
- profile reload and app switch cause binding recomputation in one place
- runtime behavior for app-specific rules is easier to trace

### Notes

This is the key bridge between today's profile model and a future explicit runtime FSM.

## Phase 3: Split Reducer and Effect Runner

### Goal

Make transitions testable without SDL, Enigo, or file watchers.

### Actions

- define an `Effect` enum for side effects such as key presses, taps, releases, rumble, logging, and timer scheduling
- update domain handlers to return `Vec<Effect>` instead of directly invoking output behavior
- narrow `ActionRunner` so it becomes an effect interpreter
- add unit tests for reducer behavior on representative event sequences

### Definition of Done

- domain logic can be tested as pure transition logic
- side effects are executed only by a dedicated effect layer
- button/stick behavior is easier to reason about from tests

### Notes

This phase delivers the biggest architectural payoff.

## Phase 4: Replace Local Scheduling Flags with Timer Events

### Goal

Turn timing into explicit runtime input instead of implicit state in the main loop.

### Actions

- define a `TimerEvent` enum
- model repeating and ticking using named timer events
- move timer decisions out of ad hoc local variables such as fast-mode and next-tick bookkeeping
- make timers part of the effect system: schedule, cancel, reschedule

Example shape:

```rust
enum TimerEvent {
    StickTick,
    ButtonRepeat { controller: ControllerId, button: Button },
    StickRepeat { controller: ControllerId, side: StickSide, generation: u64 },
}
```

### Definition of Done

- repeat and tick behavior is driven by timer events
- timer lifecycle is visible in domain transitions
- `main.rs` no longer owns timer policy details

## Phase 5: Split `Gamacros` into State Slices

### Goal

Eliminate the current god-object shape.

### Actions

- identify coherent state slices such as `ProfileState`, `AppState`, `ControllerState`, `StickState`, and `RepeatState`
- move logic closer to the state it operates on
- reduce `Gamacros` to a thin facade or remove it entirely in favor of `RuntimeState`

### Definition of Done

- no single runtime struct owns unrelated concerns by default
- controller logic and app/profile logic are easier to evolve independently

## Phase 6: Introduce Explicit FSM/HFSM Types

### Goal

Make runtime modes and controller substates first-class.

### Actions

- add explicit enums for top-level runtime states
- add explicit enums for per-controller states
- promote stick behavior into nested substates if the complexity still justifies it
- keep the machine pragmatic: not every boolean needs to become a state

### Definition of Done

- runtime modes are explicit and inspectable
- controller transition rules are encoded in the type model
- nested state is used only where it improves clarity

## Suggested Module Layout

This can begin inside `gamacrosd` and later be extracted to a separate crate if needed.

```text
crates/gamacrosd/src/
  domain/
    mod.rs
    event.rs
    effect.rs
    state.rs
    reduce.rs
    binding.rs
    timer.rs
  adapters/
    mod.rs
  app/
    ...
```

If the boundary proves stable, move `domain/` into a new crate such as `gamacros-domain`.

## Practical Rules During the Refactor

- do not introduce a state machine library early
- do not mix side effects back into reducers
- do not let adapters mutate runtime state directly
- do not convert every flag into a state unless it represents a real mode
- prefer preserving behavior while improving structure in the early phases

## Success Criteria

The refactor is working if:

- `main.rs` becomes noticeably smaller and less stateful
- timer behavior becomes explainable through events and effects
- domain tests can validate transitions without real hardware or OS integration
- adding a new mode or controller behavior requires localized changes
- app/profile/controller interactions become easier to trace

The refactor is not working if:

- the number of types grows but behavior is still hard to explain
- transition logic remains scattered across unrelated modules
- state is still split across locals, helpers, and mutable orchestration objects
- FSM terminology appears in the code, but actual transitions are still implicit

## Recommended First PR

Start with the smallest structural change that unlocks the later steps.

### Scope

- add `domain/event.rs`
- define `DomainEvent`
- adapt existing input sources to emit `DomainEvent`
- keep current handlers largely intact
- reduce logic inside `main.rs`

### Why This First

It is low-risk, keeps behavior stable, and sets up all later phases without committing to a full rewrite.

## After Phase 1

Once Phase 1 lands, the next best move is Phase 2 (`BindingContext`), not immediate full FSM extraction.

That will give the runtime a clean notion of “what rules are currently active,” which is the missing conceptual layer between raw profile data and a real hierarchical state machine.
