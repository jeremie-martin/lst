# Performance Optimization Handoff

This document is a handoff for an agent that will run a performance-optimization loop on another machine.

The goal is not just "make something faster." The goal is to preserve behavior while optimizing a single performance objective in a controlled way.


## Current testing contract

Behavior preservation is enforced by the test suite.

- `cargo test` is the blind refactor gate.
- `cargo test --features internal-invariants` is the deeper implementation-sensitive suite.

This split is intentional.

- The default gate is meant to be trusted during refactors.
- It focuses on user-visible behavior and stable contracts.
- It does **not** compile the source-file unit tests in `src/`.
- Internal invariant tests still exist, but they are opt-in because they can fail after healthy refactors that preserve behavior.

For the philosophy behind this, see [testing-philosophy.md](/home/holo/prog/lst/docs/testing-philosophy.md).


## Optimization constraint

The optimization loop can only optimize **one scalar value at a time**.

That means:

- The benchmark runner may print many measurements.
- But it must also print one final score that the optimization loop can minimize.

This is not optional. The output must contain one scalar objective.


## Performance goal

The product goal is:

- very fast startup
- very smooth interaction
- low CPU usage
- low memory footprint
- a lightweight, efficient editor overall

However, the optimization loop should not try to optimize all of those at once at the beginning.

The recommended approach is to run **one optimization campaign per scenario**.

Examples:

- startup optimization campaign
- scrolling optimization campaign
- large-file editing optimization campaign
- memory optimization campaign

The first campaign should probably target **scrolling**, because that is the user-observed hotspot right now.


## Why not optimize a universal composite score first

A composite score is possible, but it introduces arbitrary tradeoffs too early.

For example, a composite score could hide a real scrolling improvement behind a small memory regression, or reward a startup improvement while leaving the main interaction hitch unchanged.

The cleaner approach is:

- choose one scenario
- choose one primary score for that scenario
- print secondary diagnostics for interpretation

Only introduce a composite score if there is a strong reason to combine multiple dimensions inside one campaign.


## Recommended first campaign

The first optimization campaign should target **real GUI scrolling**.

Reason:

- scrolling is a real user workflow
- scrolling was observed to produce CPU spikes
- scrolling exercises event routing, viewport math, wrapping, gutter/layout work, highlighting, and redraw behavior together
- this is closer to the actual product goal than a synthetic microbenchmark


## Display choice: real display vs virtual display

Do **not** make a virtual display the default choice for the primary metric.

### Real display is preferred for the primary metric

Use the real display when measuring the main scrolling score.

Reason:

- it is more representative
- it includes the real window manager / compositor interaction
- it exercises the real rendering path the user experiences
- it is the right place to judge smoothness-related regressions

### Virtual display is still useful

A virtual display is still useful in some contexts:

- containment
- automation without disturbing a desktop session
- low-friction smoke runs

But it should be treated as a secondary tool, not as the main truth source for the scrolling objective.

### Practical recommendation

If possible, run the optimization loop on a dedicated machine or spare laptop whose display session is reserved for this work.

That gives:

- representative GUI behavior
- less environmental noise
- no interference with the main desktop


## Benchmark philosophy

Before implementing any benchmark runner, step back and define the scenario precisely.

Do not jump straight into code.

The benchmark should be:

- deterministic enough to compare revisions
- representative enough to matter
- simple enough that the score is interpretable
- narrow enough that the optimization loop has a clear target


## Recommended shape of the benchmark output

The benchmark runner should print:

1. Rich diagnostics for humans
2. One final scalar score for the optimization loop

Example output shape:

```text
startup_ms=...
trace_wall_ms=...
user_cpu_ms=...
sys_cpu_ms=...
peak_rss_mb=...
score=...
```

Possible extra diagnostics:

- frame-related timing if available
- page fault counts
- context switches
- `perf stat` counters
- file size / line count / wrap mode / syntax mode used in the run

The final `score=` line should be easy to parse.


## Recommended first score

For the first campaign, the recommended primary score is:

- **median wall-clock time of a fixed real-display scroll trace**

This is the simplest useful score.

It is better than starting with a multi-factor score because it keeps the objective clear.

Secondary values should still be printed:

- CPU time
- RSS
- startup time

But they should not be part of the optimization target yet unless there is a deliberate decision to change the objective.


## If a composite score is required

If the loop absolutely must optimize a broader notion of performance inside one campaign, use a fixed composite score with explicit normalization and fixed weights.

Example:

```text
score =
  (trace_wall_ms / wall_baseline)^0.7 *
  (cpu_ms / cpu_baseline)^0.2 *
  (rss_mb / rss_baseline)^0.1
```

This is still second-best compared to a single-scenario single-metric score.

Use a composite only if there is a clear reason and the weights are treated as part of the benchmark contract.


## Why real input matters

The benchmark should prefer **real input events** over internal message shortcuts when evaluating the primary GUI metric.

For scrolling specifically, that means:

- prefer actual wheel-scroll events or the closest real GUI event injection available
- do not use only `PageDown` / `PageUp` if the real user complaint is mouse-wheel scrolling

The point is to measure the path the user actually triggers.


## What the benchmark runner should probably control

Before coding, the agent should define the benchmark scenario completely:

- file used for the run
- file size and content characteristics
- syntax mode / highlighting mode
- wrap on or off
- window size
- font / scale assumptions if relevant
- exact input trace
- warmup policy
- number of repetitions
- cooldown / delay policy between repetitions
- whether the metric is median, mean, p95, or best-of-N

These choices matter more than benchmark code style.


## Candidate benchmark scenarios

These are examples. Pick one campaign first.

### 1. Scroll trace

Open a fixed file, place the window at a fixed size, run a fixed wheel-scroll trace down and back up, and measure total trace time plus CPU and RSS.

This is the recommended first campaign.

### 2. Startup

Launch the app with a fixed file and measure time until the window is ready enough for the scripted interaction to begin.

This is useful, but should probably be a separate campaign.

### 3. Large-file editing trace

Open a large file, click in a fixed location, perform a fixed sequence such as select-all / cut / paste / undo / redo, and measure total scenario time.

### 4. Memory-focused scenario

Open a large file, perform a fixed sequence of edits and undo/redo operations, and measure peak RSS or a similar memory metric.


## Current repo context relevant to performance work

There is already a deferred performance note in [PERFORMANCE_DEFERRED.md](/home/holo/prog/lst/PERFORMANCE_DEFERRED.md).

That document mentions these likely future areas:

- delta-based undo history
- incremental line editing for Vim and helpers
- incremental find index maintenance
- incremental wrapped layout cache updates
- syntax highlighting profiling and caching
- eventual removal of the multi-click drag workaround once upstream permits it

That document is not the benchmark plan, but it is useful context for likely hotspots once profiling begins.


## Tools and environment notes

On the machine where the agent runs, it should probe the environment instead of assuming it matches another host.

Things worth probing:

- whether a real display is reachable
- whether input injection tools are available
- whether `perf` is available
- whether the machine is quiet enough for repeated runs

If the machine is dedicated to the optimization loop, that is ideal.


## Recommended workflow for the laptop-side agent

1. Confirm the machine can launch the real GUI on its normal display.
2. Confirm available tooling for event injection and measurement.
3. Define one benchmark scenario completely before writing code.
4. Implement a benchmark runner that prints rich diagnostics and one final `score=...`.
5. Validate that the score is stable enough across repeated runs on the same revision.
6. Use `cargo test` as the behavioral constraint during optimization.
7. Use profiling tools to understand hotspots before applying performance changes.
8. Run `cargo test --features internal-invariants` as a secondary confidence pass when useful, but not as the blind gate.


## Explicit non-goals for the first pass

Do not start with:

- a huge multi-scenario performance framework
- a universal score intended to represent every aspect of editor quality
- a benchmark that depends mainly on synthetic internal messages instead of real GUI input
- optimization before there is a stable measurement target


## Recommended initial decision set

If the next agent needs a concrete starting point, use this unless the laptop environment makes it impossible:

- primary campaign: scrolling
- display: real display
- input style: real injected wheel scrolling if feasible
- main score: median wall-clock time of a fixed scroll trace
- printed diagnostics: startup, wall time, user CPU, system CPU, peak RSS
- correctness gate: `cargo test`


## Status of decisions

These points are effectively decided:

- behavior preservation is guarded by `cargo test`
- the optimization loop needs one scalar score
- the first performance campaign should likely focus on scrolling
- the primary metric should be measured on the real display if feasible
- richer diagnostics should be printed even if only one score is optimized

These points still need explicit definition before implementation:

- exact file corpus
- exact scroll script
- exact measurement method
- exact repetition policy
- exact `score=` formula
- whether CPU and RSS are diagnostic-only or part of the objective


## Final instruction to the next agent

Do not begin by writing a benchmark runner.

Begin by designing the benchmark contract:

- scenario
- environment
- input trace
- output format
- single score

Only then implement the runner.
