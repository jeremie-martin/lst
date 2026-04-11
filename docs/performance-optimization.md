# Performance Notes

This repository does not currently maintain a benchmark harness or checked-in
benchmark corpus.

Performance work should stay narrow:

- start from a user-visible problem
- keep `cargo test` green
- add the smallest measurement that proves the change
- remove one-off measurement code after the decision is made

The active behavior gates remain:

```bash
cargo test
cargo test -p lst-editor --features internal-invariants
```

The old iced implementation and historical benchmark harness were removed from
the repository. Do not reintroduce a generalized benchmark framework unless a
specific performance problem needs a repeatable contract.
