# Specgate Hardening Build — Active Checklist

## P1: CLI Refactor (sequential, Vulcan high)
- [x] **P1.1**: Extract shared types to `types.rs`
- [x] **P1.2**: Extract utility functions to `util.rs`
- [x] **P1.3**: Extract severity helpers to `severity.rs`
- [x] **P1.4**: Extract `load_project()` to `project.rs`
- [x] **P1.5**: Extract `analyze_project()` to `analysis.rs`
- [x] **P1.6**: Extract blast radius helpers to `blast.rs` [CHECKPOINT: mod.rs 2964 lines — close]
- [x] **P1.7**: Expand `validate.rs` with handler
- [x] **P1.8**: Expand `check.rs` with handlers
- [x] **P1.9**: Expand `init.rs` with handler + helpers
- [x] **P1.10**: Extract baseline command to `baseline_cmd.rs`
- [x] **P1.11**: Consolidate doctor logic into `doctor.rs` [CHECKPOINT: mod.rs 125 lines ✅]
- [x] **P1.12**: Final trim + test module [CHECKPOINT: mod.rs 118 lines ✅]
- [x] **P1-review**: Opus code review of P1

## P3: Adversarial Zoo (parallel with P1, Vulcan high fixtures, Vulcan medium runner)
- [x] **P3.1**: Fixture batch 1 (catchable scenarios)
- [x] **P3.2**: Fixture batch 2 (known gaps)
- [x] **P3.3**: Integration test runner
- [x] **P3.4**: Gap documentation

## P4: SARIF Output (parallel with P1, Vulcan medium)
- [x] **P4.1**: SARIF formatter in verdict::format
- [x] **P4.2**: Wire --format sarif into check command
- [x] **P4.3**: GitHub Actions example + docs

## Final
- [x] **Gate**: Full merge gate on master after all branches merge
- [x] **Final review**: Opus P1 review applied, merge conflicts resolved
