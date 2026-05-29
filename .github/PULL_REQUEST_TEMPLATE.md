## Summary

<!-- One paragraph: what changes and why. Reviewers read this first. -->

## Linked issue

Closes #<!-- issue number -->

<!-- If this PR doesn't close an existing issue, explain why in Summary. -->

## Crates touched

- [ ] taktora-executor
- [ ] taktora-executor-tracing
- [ ] taktora-connector-core
- [ ] taktora-connector-host
- [ ] taktora-connector-transport-iox
- [ ] taktora-connector-codec
- [ ] taktora-connector-ethercat
- [ ] taktora-connector-zenoh
- [ ] taktora-connector-can
- [ ] taktora-log
- [ ] taktora-log-dlt
- [ ] taktora-bounded-alloc
- [ ] taktora-replay
- [ ] examples / docs / CI only

## Type of change

- [ ] bug fix
- [ ] feature
- [ ] refactor
- [ ] docs
- [ ] test
- [ ] perf
- [ ] build / CI
- [ ] RFC implementation
- [ ] dependency bump

## Tests

- [ ] Added or updated unit tests
- [ ] Added or updated integration tests (workspace `*-tests` crate)
- [ ] N/A — explain in Summary

CI invocation:
```
cargo test --workspace --all-features -- --test-threads=1
```

## Docs

- [ ] Updated relevant `spec/` documents
- [ ] Updated crate-level `README.md` / rustdoc
- [ ] N/A — explain in Summary

<details>
<summary><strong>Expand if this PR adds or modifies <code>unsafe</code></strong></summary>

- [ ] New / modified `unsafe` blocks document SAFETY invariants in a `// SAFETY:` comment
- [ ] Soundness reasoning covered in PR description
- [ ] Checked with Miri where feasible

</details>

<details>
<summary><strong>Expand if this PR adds a new connector or codec</strong></summary>

- [ ] Implements the `Connector` trait
- [ ] Ships a mock back-end alongside the real one
- [ ] Health surface (`subscribe_health`) wired in
- [ ] Real-bus / session path gated behind a `*-integration` Cargo feature

</details>

## Release notes

release-plz consumes Conventional Commits. Make sure the merge commit (or squashed PR title) starts with one of:

`feat:`, `fix:`, `docs:`, `refactor:`, `perf:`, `test:`, `build:`, `ci:`, `chore:` — optionally scoped, e.g. `feat(connector-ethercat): …`.

## Self-review

- [ ] Ran `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] Ran the test suite locally
- [ ] Pre-commit hooks pass
