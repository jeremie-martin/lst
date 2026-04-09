# Performance Optimization Workflow

This repository now has one concrete first performance workflow.

The goal is simple:

- preserve behavior
- run one fixed benchmark scenario
- optimize one scalar score
- use the other printed values as diagnostics

## Current benchmark

Scenario:

- real-display X11 benchmark
- real injected wheel input via XTEST
- file: `src/app.rs`
- wrap: on
- highlighting: default Rust highlighting
- visible 3-second scroll trace: `1.5s` down, `1.5s` up
- `1s` sleep between repetitions
- `1` priming run, `7` measured runs

Runner:

```bash
cargo build --release --bin lst --bin bench_scroll_x11
./target/release/bench_scroll_x11
```

## Which value to optimize

Use the final line:

```text
score=...
```

The current contract is:

```text
score = median(cpu_ms over the 7 measured repetitions)
cpu_ms = user_cpu_ms + sys_cpu_ms
```

Lower is better. The optimization loop should minimize `score`.

This is the right current score because the main concern is not that scrolling looks choppy on this machine. The main concern is that a lightweight text editor should not burn unnecessary CPU during a simple real scrolling workload.

## Other printed values

The runner also prints diagnostics:

- `startup_ms`
- `trace_wall_ms`
- `scroll_overrun_ms`
- `user_cpu_ms`
- `sys_cpu_ms`
- `cpu_ms`
- `peak_rss_mb`

Use them for interpretation, not as the optimization target.

In particular:

- `scroll_overrun_ms` is the guardrail for smoothness and backlog
- `peak_rss_mb` is the memory diagnostic
- `startup_ms` is useful context but not part of this campaign

## Behavior-preservation gate

The blind refactor gate is:

```bash
cargo test
```

Optional secondary confidence pass:

```bash
cargo test --features internal-invariants
```

The default rule is: do not trust a performance change unless `cargo test` stays green.

## Intended edit scope

Production optimization work should primarily touch files under `src/`.

It is also acceptable to edit:

- `src/bin/bench_scroll_x11.rs` if the benchmark itself needs refinement
- `README.md`
- `docs/`

Do not broaden the project into a multi-scenario benchmark framework yet. Keep the workflow narrow and simple.

## Practical reading of results

When evaluating a change:

1. Run the benchmark.
2. Look at the final `score=...` line.
3. Prefer lower `score`.
4. Check that `scroll_overrun_ms` did not get materially worse.
5. Check `peak_rss_mb` for obvious regressions.
6. Run `cargo test`.

That is the current optimization contract.
