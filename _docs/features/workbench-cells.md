---
id: feat-workbench-cells
title: Cell documents — notebooks and runbooks
kind: feature
status: draft
summary: The shared cell layer behind Jupyter notebooks (`.ipynb` on a native kernel) and runbooks (`*.runbook.md` cells run as fresh processes) — the `kubiq-cells` vocabulary, the host workers that own each open document, the message families, the always-served MCP servers and their run gates, binding an agent to a document and the doc queue that tells it, the cell viewers with their zoom dialog and Jupyter's keymap; then what each kind does differently.
read_when: you are changing a notebook or a runbook — the document model, a kernel or a cell run, the run policy, an MCP tool, the bound agent, the cell viewer, its outputs, the zoom dialog or the cell keymap
updated: 2026-11-16
code_anchors: [crates/kubiq-cells/src/lib.rs, crates/kubiq-cells/src/process.rs, crates/kubiq/src/state/explorer/templates.rs, crates/kubiq/src/ui/cells/settings.rs, crates/kubiq-notebook/src/lib.rs, crates/kubiq-notebook/src/nbformat.rs, crates/kubiq-notebook/src/doc.rs, crates/kubiq-notebook/src/env.rs, crates/kubiq-notebook/src/wire.rs, crates/kubiq-proto/src/notebook.rs, crates/kubiq-host/src/notebook/mod.rs, crates/kubiq-host/src/notebook/kernel.rs, crates/kubiq-host/src/env_resolver.rs, crates/kubiq-host/src/cells.rs, crates/kubiq-host/src/mcp/notebook.rs, crates/kubiq-host/src/plan/queue.rs, crates/kubiq/src/ui/cells/mod.rs, crates/kubiq/src/ui/cells/zoom.rs, crates/kubiq/src/ui/cells/keys.rs, crates/kubiq/src/ui/notebook/mod.rs, crates/kubiq/src/state/notebook.rs, crates/kubiq-runbook/src/lib.rs, crates/kubiq-runbook/src/graph.rs, crates/kubiq-runbook/src/service.rs, crates/kubiq-runbook/src/shell.rs, crates/kubiq-proto/src/runbook.rs, crates/kubiq-host/src/runbook.rs, crates/kubiq-host/src/mcp/runbook.rs, crates/kubiq/src/ui/runbook/mod.rs, crates/kubiq/src/state/runbook.rs]
depends_on: [feat-workbench-ide, tech-transport, tech-architecture]
---

# Cell documents — notebooks and runbooks

## Purpose

Two kinds of document are made of cells that run: a **notebook** — a Jupyter `.ipynb` whose code
cells run on one persistent kernel, outputs saved in the file — and a **runbook** — a markdown
`*.runbook.md` whose fenced cells run as fresh processes, with an evidence log. They keep opposite
state models on purpose (a kernel's hidden state against a reproducible log), and share everything
else: the vocabulary, who owns the document, how a window and an agent reach it, how an agent's
run is gated, who is told when something happens, how a cell and its output are drawn, zoomed and
driven from the keyboard. This document states the shared layer once; the two kind sections at the
end say only what differs. Both open in the IDE's editor tabs ([the IDE](./workbench-ide.md)).

## Behaviour

- **Opening.** A `.ipynb` opens in the Notebook viewer, a `*.runbook.md` in the Runbook viewer.
  The host reads the document; a notebook has no size cap (a file full of plots opens whatever its
  size).
- **One document, many viewers.** Two tabs, two windows and any number of agents work on the same
  cells. An edit from any of them reaches every open view at once. An agent's edit is marked with
  the agent's name on the cell (the origin badge); a cell a person is typing in is never
  overwritten under the cursor.
- **Running.** A person runs a cell from its row or the keyboard. Outputs stream into the row as
  they arrive, for every viewer, whoever asked for the run. A cell shows its run state by colour:
  queued, running, ok, error, stopped, stale, or *waiting for you* (an agent's run held for a yes).
  **Stale** marks a cell whose source changed since it ran, or that ran before a cell above it ran
  again — a mark, never a block.
- **Agents.** Every hosted agent can read, edit and run cells through the kind's MCP server
  (`kubiq-notebook`, `kubiq-runbook`), served to every agent whatever its definition names. An
  agent's run passes the document's **run gate**; under the default it becomes a question —
  *Allow*, *Allow for this session*, *Deny* — both in the viewer's header and as the agent's own
  ask dialog, raised when its turn ends; the first answer wins, and the agent is told it is
  pending meanwhile. **An approval covers the code it showed**: a notebook question records each
  cell's source, *Allow* runs only if every cell still holds it (otherwise the question stays open
  with a note), an approved run whose cell is edited before it starts does not run, and an agent
  cannot edit a cell while a run of it waits. *Allow for this session* covers the asking agent
  alone, until the kernel restarts.
- **Nothing in the project runs on opening it.** A notebook lists the project's own interpreters
  by path; they are run (to probe for `ipykernel`, or `uv python find`) only after *Detect
  kernels*, a consent that lasts for the host's run. Picking a kernel runs that one. Without the
  consent a run with no kernel asks for a pick rather than starting a default.
- **The bound agent.** The header's *Bound* control picks which of the project's agents is *told*
  about the document — binding never limits who may use it. *New agent…* there opens the new-agent
  dialog and binds the agent it starts. A question a person asks about a cell,
  a run they approved or declined, and a run the agent asked for finishing are queued for the bound
  agent and delivered at its next turn boundary, in the same one prompt as its annotated documents'
  comments (`D207`).
- **Outputs.** Text (stdout, stderr, `text/plain`, tracebacks) is drawn with ANSI codes removed
  and `\r` progress lines collapsed, selectable and copyable in place. PNG, JPEG, GIF and SVG draw
  as images. `text/markdown` renders. An HTML table falls back to its `text/plain`; any other rich
  type shows a placeholder naming its MIME types beside its plain text.
- **Zoom.** Every text output carries a zoom button; an image zooms on a click; `F` zooms the
  selected cell's outputs (or its source when it has none). The dialog fills most of the window:
  text in a read-only editor with line numbers, find, selection and copy, *Copy all* in its header;
  an image fitted to the window. Esc, ×, or a click outside closes it.
- **Saving.** ⌘S on the tab writes the document through the host. A notebook is written the way
  Jupyter writes it, and an unedited notebook re-saves byte for byte, whichever tool wrote it.
- **Creating one.** The explorer's right-click offers *New file*, *New folder*, the drawings,
  *New notebook* and *New runbook* — flat while there are at most four *New* entries, folded into
  *New ▸* beyond that (today six, so folded). A typed entry asks for a name seeded `Untitled.ipynb`
  (`Untitled 2.ipynb` when taken), writes a valid empty document, and opens it in its viewer.
- **Preferences.** Settings › System › *Cells*: the agent policy a new notebook starts with, saving
  notebooks without outputs, how many lines of a text output a cell draws; the runbook's rows
  follow.
- **Outside changes.** A clean document re-reads a file changed on disk, keeping each cell's state
  by id; a document with unsaved edits says the file changed and keeps the edits.

### Keyboard

One key context, `CellsView`, for both kinds; Jupyter's keys, so muscle memory transfers.
**Command mode** (Esc; the list has focus) and **edit mode** (Enter; a cell's editor has focus):
letters act on cells only in command mode, so an editor never loses a key.

| Keys | Does |
|---|---|
| Shift-Enter | Run, select the next cell (adds one at the end). Edit mode too |
| Ctrl/Cmd-Enter | Run in place. Edit mode too |
| Alt-Enter | Run, insert a cell below. Edit mode too |
| Esc / Enter | Command mode / edit the selected cell |
| A / B | Insert above / below |
| D, D | Delete the cell |
| M / Y / R | Markdown / code / raw (runbook: prose / cell) |
| J, K, ↑, ↓ | Previous / next cell |
| C / X / V | Copy / cut / paste a cell |
| Z | Undo the last cell operation |
| I, I | Interrupt (runbook: Stop) |
| 0, 0 | Restart the kernel (notebook only) |
| O | Hide or show the outputs |
| F | Zoom the cell's outputs or source |
| Ctrl/Cmd-S | Save |

For a runbook the run keys approve or run per its gate: still a human key press, and `confirm=`
still asks.

### Checking it by hand

The viewer cannot be exercised from an agent shell; these are the manual checks.

1. Open a JupyterLab-saved, a VS Code-saved and a Colab-saved `.ipynb`; save each untouched; `git
   diff` shows nothing.
2. Open a notebook over 2 MiB (a few large plots): it opens, scrolls and saves.
3. Open a notebook in a fresh clone: nothing in the project runs (no `python` process) until
   *Detect kernels*. Pick a kernel from the header: the project's `.venv` is first; a uv project without `ipykernel`
   shows the three offers, and *Run without installing* starts a kernel with nothing written.
4. Shift-Enter through `print(1+1)`, `40+2`, a PNG display, `1/0`, `input("x? ")`: outputs, the
   count gutter, the error in danger colour, the inline prompt.
5. `import time; time.sleep(30)` then I, I: the run ends as stopped; 0, 0 restarts; *Restart and
   run all* reruns top to bottom.
6. Edit a cell above one that ran, run it: the one below shows *stale*.
7. Select text in an output and copy it; zoom a 5 000-line output: line numbers, find, Copy all,
   Esc closes; click a plot: it fits the window.
8. With the policy on *ask*, have an agent call `run_cells`: the header asks; *Allow for this
   session* lets its next run through until a restart; *deny* refuses.
9. Bind an agent; ask about a cell; approve its run: the agent receives one prompt carrying both.
10. Two windows on one notebook: an agent's edit appears in both, the badge names the agent, the
    cell being typed in keeps its cursor.
11. Every key in the table above, in command and in edit mode; typing `a`, `d`, `0` into a cell
    types them.
12. Cross-OS: the UI on macOS attached to a host on Windows — a `Scripts\python.exe` venv is found,
    interrupt stops a run, the kernel's connection file sits under that host's config root.
13. Runbook: open a runbook with two parallel branches and a join (
    each step with a check): it opens in the runbook view, not Markdown; Run ready (1) runs the root;
    its check is offered next; two branches run in parallel, each with its own live tail and
    "N lines skipped" past 15 lines; a failing check blocks the join.
14. Runbook gate: an agent's `request_run` shows the bar with the substituted command (secrets as
    •••) and its name; editing the cell (unsaved) disables Run; saving and Run runs; a waiting step
    asks "without its prerequisites"; a `confirm=` cell asks twice.
15. Runbook evidence: Stop a `sleep 30` cell (cancelled); a `timeout=1s` cell (timed-out); quit
    mid-run and reopen (interrupted); skip and override with a reason (both written under the
    cell); a manual check's Pass with a note; *Open the full output* of a 100 000-line run.
16. Runbook selection: select and copy text from a command, an excerpt and the zoom dialog.
17. Runbook agent: bind an agent; run a cell; the agent's next turn names the finished cell.
18. Runbook cross-OS: on a Windows host, a bare fence runs under `pwsh`, ```` ```cmd ```` under
    `cmd`, Stop ends a child tree, `$env:RUNBOOK_ENV` carries a key to the next cell; a missing
    interpreter is a lint, not a failed run.
19. Right-click the empty explorer, a folder and a file: *New ▸* lists New file, New folder, the
    drawings, New notebook, New runbook; *New notebook* twice gives `Untitled.ipynb` then
    `Untitled 2.ipynb`, each opening in its viewer with one empty cell.
20. Settings › System › *Cells*: set the notebook policy to *deny*, open a new notebook — an
    agent's `run_cells` is refused; tick *Save without outputs*, save — the file has no outputs and
    the open notebook still shows them; pick 100 lines — a long output folds after 100.
21. Under *ask*, an agent's `run_cells` raises the ask dialog when its turn ends; *Allow* runs the
    cells and the header's question disappears; answering in the header first leaves the dialog
    with nothing to do.
22. *Bound* › *New agent…*: start one from the dialog — the chip names it without a second pick.
23. On a Windows host: interrupt a `time.sleep(30)` cell (the `JPY_INTERRUPT_EVENT` path), and
    Restart a kernel that started a child process — no orphan python remains.
24. Runbook on a drone: open a project a drone serves (a Linux drone, then a Windows one, from a
    macOS window); run a cell — it runs on the drone (`hostname` names it), its live tail and output
    block arrive, Stop ends it; quit the drone mid-run and reattach — the attempt is interrupted. A
    drone of an older build shows "too old to run runbooks" instead of waiting.
25. Runbook over ssh: a cell `{#up target=ssh:<alias>}` runs `uname -a` on that host through the
    host's own ssh config (a key, no prompt: BatchMode); `target=ssh:<alias>@windows` with a bare
    fence runs `Get-Date` under PowerShell there; a host without `ssh` shows a lint.

## Contract

- **Shared vocabulary**, `kubiq-cells`: `Origin` (human, `agent:<id>`, disk), `RunStatus`,
  `RunState` (a row's state), `RunMarks` (staleness), `CellDocument` (cells in order, their
  source and kind), `RunGate` / `GateDecision` (run, ask, refuse), `DocEvent` (an event for the
  bound agent) — re-exported through each kind's message records.
- **Messages.** One pair per kind, by project and project-relative path: `Notebook` /
  `NotebookEvent` (`kubiq-proto/src/notebook.rs`) and `Runbook` / `RunbookEvent`. A snapshot answers
  the window that opened; every change goes to every window. `QueueDocEvent` — from a window or
  from the host's own workers — puts a `DocEvent` on the bound agent's doc queue. The rows are in
  [the transport contract](../tech/transport-contract.md).
- **MCP.** `kubiq-notebook` and `kubiq-runbook` are in `catalogue::ALWAYS_MCPS`; each is a
  `ServiceReach` answered by its worker on a thread of its own.

## Implementation

- **Shared, host.** `kubiq-host/src/cells.rs` holds `CellExecutor`, the shape of what runs a cell
  (the notebook's kernel implements it; a runbook runs cells in parallel processes and keeps its
  own service). `kubiq_cells::process` (feature `process`) starts a cell's process as a group and
  interrupts or kills it as one: a process group and signals on Unix; on Windows a Job object
  (tree kill), `CTRL_BREAK_EVENT`, and for a Jupyter kernel the inheritable event handed over in
  `JPY_INTERRUPT_EVENT`. An agent's gated run is also armed as its `ask_user_question` dialog
  (`Message::ArmCellGate`); the coordinator hands the answer to the document's worker
  (`decide_cell_gate`). The doc queue (`plan/queue.rs`, `DocQueue::push_event`) carries document events
  beside comment threads; the coordinator's `QueueDocEvent` arm queues and delivers.
- **Shared, interface.** `ui/cells/`: `zoom` (the dialog, one `Global`, drawn by the hosting
  viewer last), `output` (selectable text block, image cache, markdown, placeholder), `agent` (the
  bound-agent control), `keys` (the context, the actions, the bindings registered after
  `gpui_component::init`), `run_chip` (state colour), `settings` (the *Cells* section;
  `state::cells::CellsSettings` in the interface's settings). The explorer's typed *New* entries
  are `state::explorer::templates`.
- **Environment resolver**, `kubiq-host/src/env_resolver.rs` over `kubiq_notebook::env`: which
  interpreter runs code for a file. `.venv/` then `venv/` walking up to the project root (`bin/python`
  or `Scripts\python.exe`), then uv (`pyproject.toml` + `uv.lock` → `uv python find`, bounded at
  5 s), then the machine's Jupyter kernelspecs (listed, never written); a manual pick. The
  `import ipykernel` probe is cached per interpreter. A kernelspec is synthesised
  (`<python> -m ipykernel_launcher -f {connection_file}`); a uv project lacking `ipykernel` also gets
  `uv run --no-sync --with ipykernel …`. Nothing is installed without a click (`EnvAction`).

### The notebook

- **Engine**, `kubiq-notebook` (a leaf): `json` keeps key order and number text so a file prints
  back in its writer's style (indent width read off the file, sorted keys kept sorted, new keys
  appended to an unsorted object); `nbformat` keeps each cell's raw object and overwrites only the
  fields it owns; ids minted for pre-4.5 files are written, with `nbformat_minor` raised to 5, only
  once the notebook is edited; `doc` holds id-addressed `CellOp`s, the notebook undo stack (a
  person's typing is the cell editor's own history; an agent's source edit is one notebook step),
  run marks and reload by id; `wire` is the Jupyter message (HMAC-SHA256) and ZMTP 3.0 framing.
- **Kernel**, `kubiq-host/src/notebook/kernel.rs`: Kubiq's own client of the wire protocol over
  blocking loopback sockets (`zmtp.rs`), a reader thread per channel (`D239`). Connection file at
  `<config root>/runtime/kernels/kernel-<id>.json`, mode 0600, swept at start; the argv in its own
  process group, cwd the notebook's folder; ready is a heartbeat plus `kernel_info_reply` with iopub
  proven live; interrupt is SIGINT to the group, else `interrupt_request`; shutdown asks on control,
  waits 3 s, kills.
- **Worker**, `kubiq-host/src/notebook/mod.rs`: documents keyed by (project, path), at most one
  kernel each; kernel output applied in batches; a run queue per document; the per-notebook agent
  policy (`ask`, `allow`, `deny`) in the project's `local/notebook-policy.json`; the bound agent;
  outside changes checked once a second.
- **Viewer**, `ui/notebook/mod.rs` and `state/notebook.rs`: a contributed viewer the base
  registers, drawing from its own mirror of the host's cells, never the tab's buffer; ⌘S routes to
  the host (`AppState::save_file`).

### The runbook

- **Document.** A `*.runbook.md` is the project's own markdown file; the editor tab reads, saves
  and (`live_reload`) re-reads it, and Source is the buffer. A fence with an attribute group
  `{#id needs=a,b check-of=x readonly confirm="…" timeout=30s shell=… retry=NxT expect="re"}` is a
  cell; a ```` ```check {… manual} ```` block is a manual check; a plain fence is an example.
  `kubiq-md` keeps only a fence's language, so `kubiq_runbook::format` scans fences itself, by byte
  span, and a rewrite touches only the blocks it owns. Frontmatter: `params` (declared; bound by a
  provider, the literal `default` otherwise), `target` (`local` only), `gate`
  (`every-cell` | `readonly-auto`).
- **Graph and states.** A step needs `needs=`, else the previous step (checks ignored); `needs=`
  empty starts a branch. Lints: missing or duplicate id, unknown need, a need on a check, cycle,
  `check-of` a check, bad attribute, unbound parameter, an interpreter this host lacks, a
  non-local target, `interactive`. An error blocks the cells it touches, nothing else. States —
  waiting, ready, running, checking, done, failed, skipped, overridden, cancelled, timed-out,
  interrupted, stale, invalid — are derived on every read from the text, the bindings and the
  evidence, never stored (`graph.rs`). A step is done when its latest attempt exited 0, every check
  of *that attempt* passed and the attempt is fresh: same command hash, same prerequisite proofs. A
  prerequisite's edit or rerun makes every dependent and its manual verdicts stale.
- **Evidence.** Append-only JSON lines (`Started`/`Finished`/`Verdict`/`Decision`) beside
  `logs/<cell>-<attempt>.log` and `env` (`$RUNBOOK_ENV`) in `ProjectData::runbooks()/<doc>/` —
  this machine's local data, never in the project. An unfinished attempt found on open is closed
  as interrupted. The host projects the evidence into `output` blocks under each cell (latest
  excerpt, earlier runs one line each, verdicts, skips, overrides), idempotently; an `output` block
  anyone else writes is replaced, one for a cell with no evidence is left alone.
- **Runs.** A fresh process per cell, no terminal, its own process group (`taskkill /T /F` on
  Windows), the interpreter by fence language and the host's OS (`shell.rs`: `sh` on Unix, `pwsh`
  then Windows PowerShell on Windows; `pwsh -NoProfile -NonInteractive -Command -`, `cmd /d /s /c`,
  UTF-8 forced; the command is an owner-only script file removed after the run, never an
  argument, so no value reaches the process table), cwd the project root, `$RUNBOOK_ENV`'s `KEY=value` lines (CRLF and a BOM
  tolerated) loaded. Bound secrets are masked across chunk boundaries before a byte is logged,
  streamed or kept; the log stops at 16 MB with a marker; the excerpt is the first and last 20
  lines with real numbers, under 8 KB; the live tail is batched every 40 ms, at most 15 lines a
  frame. Timeout and Stop end the group (TERM, then KILL after 2 s). Cells run in parallel; output
  blocks are written one at a time.
- **Where it runs.** On the host that serves the project: this machine, a full remote host
  (`--serve`), or **the drone** serving it — the drone's relay answers `Runbook` with the same
  worker (lean `kubiq-host`), so cells run, are captured, masked and logged on the drone and only
  messages cross the carrier; it advertises `CAP_RUNBOOK`, and a window refuses a runbook on a
  drone without it (`D243`). A cell's `target=ssh:<alias>[@windows]` (an SSH-config alias of letters, digits and `-_.`;
  `user@host` is a lint) runs through the host's
  `ssh` (`-T`, `BatchMode`), the interpreter chosen for the far OS, the script on stdin; the
  target is part of the approved hash. `drone:<id>` on a cell is a lint: open the project from the
  drone. Evidence and log references are relative `/` paths, the same on every OS.
- **Gate.** The agent files `request_run` / `request_check` and returns; the viewer shows a
  Run · Edit · Skip bar with the command as it would run and who asked. A run carries the hash (secrets named as `{{secret:<name>}}`, never valued) of
  the command the person saw; a different hash (an edit, an unsaved buffer) is refused. A waiting
  step runs only after "run without its prerequisites" (recorded as `forced`), a `confirm=` cell
  after its second yes. `gate: readonly-auto` (`gate.rs`, the shared `RunGate`) runs an agent's
  request at once only for ready `readonly` cells a person accepted for their current text (the acceptance carries the hash
  they saw), with no
  `confirm=`, never when a parameter name, a non-secret value or a target holds `prod`, `prd`, `production` or
  `live` as a token, any case; the actor is recorded as
  `readonly-auto for <agent>`. An agent's `request_run` still waiting after that is also armed as
  the agent's ask dialog (*Allow* / *Deny*), raised when its turn ends: *Allow* runs the cells only
  if every command's hash — target included — is still the one the agent asked about
  (`service::gate_approvals`), otherwise nothing runs and the request stays pending with a note;
  *Deny* resolves it as skipped by a person. Either way the agent hears it through its doc queue.
- **Provider seam.** `kubiq_host::runbook::Registry` on `Contributions::runbook_providers` (empty
  in the base): a `Provider` binds parameters (and secrets) for a document, may make it read-only (`locked`) or
  editable but not runnable (`idle`), is told
  every record, and offers actions the viewer draws as toolbar buttons with a small form. An
  edition's templating and history (master and instance, run history) live behind it.
- **Settings.** The Cells section's runbook rows (`ui/cells/settings.rs::runbook_rows`): the log
  cap and the timeout of a cell that names none, sent with each run (`RunOptions`).
- **Viewer**, `ui/runbook/mod.rs` and `state/runbook.rs`, claimed over the Markdown view with
  `Claim::Specific`. Header: role, gate, Run ready (n), Stop, the provider's actions, parameters,
  the bound agent, a notice, the confirmation bar, an action form, a provider report. Rows: prose
  rendered; per cell its id, kind, state, needs, checks, command (selectable, zoom), controls
  (Run, Stop, Skip, Override, Pass/Fail with a note, Accept readonly), an agent's request bar, the
  live tail, and each output block with its facts and *Open the full output* (the retained log up
  to 2 MB in the zoom dialog). Keys: Shift/Ctrl-Enter approve and run the selected cell (answering
  its pending request), `I I` stops it, F zooms its command.

## Failure

- A kernel that will not start, or dies, shows *dead* with the last of what it printed; queued and
  running cells end as stopped.
- A run with no kernel starts the default candidate; with none that has `ipykernel`, the header
  says so and offers the choices.
- An unparseable notebook opens nothing and says why; a project served by a drone answers that
  kernels run on the host's own machine.
- An agent run under `deny`, or an interrupt or restart under anything but `allow`, is refused
  with the reason.

## Related docs

- [The IDE](./workbench-ide.md) — the editor tabs these viewers draw in, `D204`'s viewer seam.
- [The transport contract](../tech/transport-contract.md) — the message rows.
- [The decision register](../tech/decisions.md) — `D239`, `D240`, `D241`, `D242`, `D243`.
- [The backlog](../backlog.md) — what is still open here.

## Next steps

- Virtualise the cell list (`gpui::list`) and debounce a cell's edits to the host.
- The notebook-pin and project-kernel rows of the resolver, with `D230` trust.
- Command-palette entries *New notebook* / *New runbook*.
