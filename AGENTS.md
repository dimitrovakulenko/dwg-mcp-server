# AGENTS.md

This file is the operating guide for AI coding agents working in this repository.
It applies to the whole repository unless a more specific `AGENTS.md` exists in a
subdirectory. The vendored LibreDWG tree already has its own instructions, so do
not edit `third_party/libredwg` unless the task explicitly requires changes
there.

## Project Snapshot

DWG MCP Server is a read-only Model Context Protocol server for inspecting
AutoCAD DWG files. The Python host exposes MCP tools, validates file access, and
starts one Rust `dwg-worker` process per opened drawing. The Rust side indexes
DWG content through LibreDWG and answers object and query requests over a JSON
line protocol.

Keep the default posture conservative: protect host file access, preserve
read-only behavior, and avoid protocol-breaking changes unless the user asks for
them.

## Repository Map

- `server/` contains the Python MCP host, path-access policy, worker process
  management, and Python tests.
- `crates/dwg-worker-core/` contains shared protocol, object models, indexing,
  query logic, and tests that do not need the native LibreDWG bridge.
- `crates/dwg-worker/` contains the stdio worker binary that serves one DWG
  document.
- `crates/dwg-libredwg/` contains the LibreDWG-backed document implementation.
- `crates/libredwg-sys/` contains the native bridge and bindgen setup. It is
  intentionally excluded from the root Cargo workspace.
- `scripts/` contains build and integration-test helpers.
- `docker/` contains container packaging support.
- `testData/` contains DWG fixtures used by local and CI tests.
- `third_party/libredwg/` is vendored upstream code. Treat it as external source.

## Common Commands

Use the smallest command that gives confidence for the change.

```bash
git submodule update --init --recursive
bash scripts/build-libredwg.sh
cargo fmt --all
cargo test --workspace
cargo build -p dwg-worker
PYTHONPATH=server/src DWG_WORKER_BIN=target/debug/dwg-worker python3 -m unittest server.tests.test_file_access server.tests.test_host server.tests.test_mcp_stdio -v
bash scripts/run-e2e-tests.sh
```

Notes:

- Build LibreDWG before native Rust tests when `third_party/libredwg/src/.libs`
  is missing.
- Docker smoke tests require Docker and should not be assumed available in every
  agent environment.
- If a dependency download or networked command is required, explain why and ask
  for approval through the available tool mechanism.

## Coding Guidelines

- Keep the MCP server read-only. Do not add write, mutate, or execute behavior
  for DWG files unless the user explicitly changes the product direction.
- Route all host-file access through the existing access-control layer. Do not
  bypass path validation from tool handlers or worker management code.
- Preserve the worker JSON protocol unless a task intentionally changes it. When
  protocol fields change, update Rust models, Python models, tool schemas,
  examples, and tests together.
- Prefer stable, serializable data shapes. Use camelCase for protocol JSON fields
  where the existing protocol does so.
- Keep DWG handles as strings, usually uppercase hexadecimal, to avoid integer
  precision or formatting surprises across clients.
- Add derived DWG data in the indexing layer rather than recomputing it for each
  query when the information is reusable.
- For geometry-like data, preserve the original numeric point values wherever
  practical. Do not replace exact contours, vertices, or bounds with only human
  summaries when a tool asks for full detail.
- Be defensive around native pointers, counts, and LibreDWG-owned memory. Check
  null pointers and suspicious lengths before converting to Rust values.
- Do not edit generated bindgen files or Cargo build outputs. Change bridge
  headers, C bridge code, or Rust wrappers instead.
- Keep refactors narrow. This project crosses Python, Rust, C, and vendored
  native code, so small changes are much easier to validate.

## Native LibreDWG Work

When exposing new DWG fields from LibreDWG:

- Add or adjust C bridge access in `crates/libredwg-sys/bridge.c` and
  `crates/libredwg-sys/bridge.h` only when the generated bindings are not
  sufficient.
- Convert native data into owned Rust values before returning it outside the FFI
  boundary.
- Surface object data through `crates/dwg-libredwg/` and keep generic object
  behavior in `crates/dwg-worker-core/`.
- Update schema or model code when new fields should be visible in `get_objects`
  or `query_objects`.
- Add tests with existing fixtures when possible. If a fixture-specific assertion
  is needed, keep it explicit and documented by the test name.

## Python Host Work

- Keep tool handlers small and let helper modules own validation, process
  lifecycle, and serialization details.
- Treat client-provided paths and handles as untrusted input.
- Keep worker startup configurable through `DWG_WORKER_BIN` so tests and local
  builds can use debug binaries.
- Avoid making MCP tool output depend on local absolute paths except where path
  reporting is already part of the API.

## Tests By Change Type

- Protocol or object-model changes: run `cargo test --workspace` and relevant
  Python MCP host tests.
- LibreDWG extraction changes: run `bash scripts/build-libredwg.sh`,
  `cargo test -p dwg-libredwg`, and at least one integration or fixture-based
  test that exercises the field.
- Python host changes: run the focused `python3 -m unittest ...` command above.
- Docker or packaging changes: run the Docker smoke test only when Docker is
  available, and say clearly if it was skipped.
- CI-only changes: validate YAML structure and keep changes minimal.

## Security And Safety

- Never weaken host directory restrictions, path canonicalization, or read-only
  assumptions to make a test pass.
- Do not log sensitive local paths more broadly than existing diagnostics do.
- Avoid shelling out from the server for request handling.
- Do not commit local build artifacts, caches, `target/`, or compiled LibreDWG
  outputs.

## Git And Review Hygiene

- Check the worktree before editing and avoid overwriting user changes.
- Keep commits focused. Documentation-only changes should be separate from code
  or CI behavior changes when practical.
- Use non-interactive git commands.
- Stage only files that belong to the task.
- In reviews, prioritize behavioral bugs, regressions, missing tests, security
  risks, and CI/release hazards over style comments.

## Release And CI Notes

- CI builds LibreDWG, runs the Rust workspace tests, builds `dwg-worker`, installs
  the Python server package, and runs Python host tests.
- The Docker publish workflow is a separate manual deployment path. Do not start
  deployment workflows unless the user asks and CI is green.
- If CI is slow because LibreDWG rebuilds, prefer cache-key or artifact reuse
  improvements keyed by the LibreDWG submodule revision rather than skipping
  native validation entirely.
