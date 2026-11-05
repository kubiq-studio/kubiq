---
id: tech-code-map
title: Code map
kind: tech
status: current
summary: Generated map of the application's source tree, and the inverted index from every file to the documents that anchor it.
read_when: you changed a file and need to know which documents owe an update, or you are looking for where something lives
updated: 2026-10-01
verified: 2026-09-23
depends_on: [tech-structure]
review_cycle: monthly
---

# Code map

Two generated blocks: the application's source tree, and the inversion of every document's
`code_anchors` frontmatter. `just docs-index` rewrites both; `just docs-check` fails if they are
stale. The descriptions beside tree entries are written by hand and survive regeneration — add one
by editing the line and running the command again.

The scope is `crates/kubiq/src`. The harness library draws its own tree in
`crates/agent-manager/AGENTS.md`, because it owns its documentation — see
[`agent-manager.md`](./agent-manager.md).

For the rules about what belongs in each of these files, read
[`project-structure.md`](./project-structure.md); this document says only where things are.

## The tree

<!-- generated:begin tree -->

```
crates/kubiq-proto/src/
├── bus.rs
├── ids.rs
├── lib.rs
├── log.rs
├── messages.rs
├── projects.rs
├── files.rs
├── work.rs
├── git.rs
├── settings.rs
├── conversation.rs
├── search.rs
├── connectors.rs
├── repos.rs
├── stats.rs
├── wire.rs
├── assist.rs
├── notifications.rs
├── tools.rs
├── mcp.rs
├── quota.rs
├── carrier.rs
├── drone/
│   ├── manifest.rs
│   └── mod.rs
├── kb.rs
├── acp.rs
├── feedback.rs
├── help.rs
├── ask.rs
├── plan.rs
├── blocks.rs
├── mission.rs
├── tasksrc.rs
├── catalog.rs
├── db.rs
├── merge.rs
├── update.rs
├── ssh.rs
├── browser.rs
├── remote_tools.rs
└── tunnel.rs

crates/kubiq-host/src/
├── pty/
│   └── mod.rs
├── agent.rs
├── coordinator.rs
├── lib.rs
├── store/
│   ├── file.rs
│   ├── memory.rs
│   ├── mod.rs
│   ├── harness.rs
│   ├── usage.rs
│   ├── plan.rs
│   ├── project_dir.rs
│   └── browser_history.rs
├── atomic.rs
├── config.rs
├── gc.rs
├── health.rs
├── projects.rs
├── files/
│   ├── mod.rs
│   ├── path.rs
│   ├── diff.rs
│   ├── browse.rs
│   ├── related.rs
│   └── ignored.rs
├── work/
│   ├── mock.rs
│   └── mod.rs
├── reply.rs
├── git/
│   ├── mod.rs
│   ├── graph.rs
│   ├── history.rs
│   ├── observe.rs
│   ├── nested.rs
│   └── write.rs
├── settings.rs
├── shells.rs
├── conversation.rs
├── search/
│   ├── ceiling.rs
│   ├── fallback.rs
│   ├── mod.rs
│   ├── walk.rs
│   ├── worker.rs
│   └── hits.rs
├── watch/
│   └── mod.rs
├── cli_shortcut.rs
├── shell_integration.rs
├── links.rs
├── connectors/
│   ├── app.rs
│   ├── flow.rs
│   ├── http.rs
│   ├── mod.rs
│   ├── providers.rs
│   ├── store.rs
│   └── tls.rs
├── index/
│   ├── ceiling.rs
│   ├── mod.rs
│   └── text.rs
├── repos/
│   ├── clone.rs
│   ├── list.rs
│   └── mod.rs
├── remote.rs
├── assist/
│   ├── apple.rs
│   ├── mod.rs
│   ├── stub.rs
│   ├── subject.rs
│   ├── api.rs
│   └── providers.rs
├── notifications/
│   ├── mod.rs
│   └── os.rs
├── host_path.rs
├── environment.rs
├── conversation_record.rs
├── host_meta.rs
├── mcp/
│   ├── catalogue.rs
│   ├── mod.rs
│   ├── registry.rs
│   ├── server.rs
│   ├── tools.rs
│   ├── tasks.rs
│   ├── kb.rs
│   ├── help.rs
│   ├── ask.rs
│   ├── plan.rs
│   ├── mission.rs
│   ├── sql/
│   │   ├── blobs.rs
│   │   ├── encode.rs
│   │   └── mod.rs
│   ├── archify.rs
│   ├── browser.rs
│   └── remote.rs
├── web_assets/
│   ├── manifest.rs
│   ├── mod.rs
│   ├── archive.rs
│   └── manifest_drawio.rs
├── quota.rs
├── carrier.rs
├── drone/
│   └── mod.rs
├── kb/
│   ├── mod.rs
│   ├── store.rs
│   ├── sync.rs
│   ├── ops.rs
│   └── vault.rs
├── feedback/
│   ├── api.rs
│   ├── issues.rs
│   ├── mod.rs
│   └── keychain.rs
├── help/
│   ├── archive.rs
│   └── mod.rs
├── ask.rs
├── plan/
│   ├── mod.rs
│   ├── blocks.rs
│   ├── lines.rs
│   ├── provenance.rs
│   ├── service.rs
│   ├── lineage.rs
│   └── queue.rs
├── mission/
│   ├── mod.rs
│   ├── prompts.rs
│   └── runner.rs
├── armed.rs
├── tasksrc/
│   ├── mod.rs
│   ├── store.rs
│   ├── sync.rs
│   ├── trello.rs
│   ├── outbound.rs
│   └── service.rs
├── catalog.rs
├── runners/
│   ├── just.rs
│   ├── make.rs
│   ├── mise.rs
│   └── mod.rs
├── db/
│   ├── jobs.rs
│   ├── mod.rs
│   ├── secrets.rs
│   ├── session.rs
│   ├── store.rs
│   ├── tests.rs
│   ├── agent.rs
│   ├── editors.rs
│   └── tunnel.rs
├── archify.rs
├── updates/
│   ├── apply.rs
│   ├── feed.rs
│   └── mod.rs
├── acp.rs
├── .DS_Store
├── browser.rs
├── drone_modes.rs
└── trust.rs

crates/kubiq/src/
├── state/
│   ├── mod.rs
│   ├── chat.rs
│   ├── editor.rs
│   ├── workbench.rs
│   ├── windows.rs
│   ├── logs.rs
│   ├── prefs.rs
│   ├── when.rs
│   ├── agents.rs
│   ├── layout.rs
│   ├── board.rs
│   ├── work.rs
│   ├── dock.rs
│   ├── scene.rs
│   ├── diagrams.rs
│   ├── sink.rs
│   ├── viewport.rs
│   ├── file_picker.rs
│   ├── git.rs
│   ├── settings.rs
│   ├── conversation.rs
│   ├── search.rs
│   ├── explorer/
│   │   ├── filter.rs  the go-to-file substring filter
│   │   ├── keys.rs    keyboard navigation
│   │   ├── menu.rs    the row context menu
│   │   ├── mod.rs     `ExplorerState`, `FileNode`, `Row`, `GitStatus`
│   │   ├── rows.rs    flattening the tree into drawable rows
│   │   └── tree.rs    listing, `merge`, expansion
│   ├── vim/
│   │   ├── mod.rs
│   │   ├── motion.rs
│   │   ├── object.rs
│   │   ├── search.rs
│   │   └── step.rs
│   ├── nav.rs
│   ├── nav/
│   │   └── text.rs
│   ├── navigator.rs
│   ├── clone.rs
│   ├── stats.rs
│   ├── image_edit.rs
│   ├── remote.rs
│   ├── notifications.rs
│   ├── new_agent.rs
│   ├── a2ui/
│   │   ├── checkable-inputs.json
│   │   ├── contact-form.json
│   │   ├── icons-and-text.json
│   │   ├── kitchen-sink.json
│   │   ├── tabs-icons-buttons.json
│   │   ├── action.rs
│   │   ├── bindings.json
│   │   ├── eval.rs
│   │   ├── live.rs
│   │   ├── path.rs
│   │   ├── svg.rs
│   │   ├── kubiq-catalog-demo.json
│   │   ├── kubiq-catalog.json
│   │   └── value.rs
│   ├── a2ui.rs
│   ├── remote_hosts.rs
│   ├── web_panel.rs
│   ├── script.rs
│   ├── kb.rs
│   ├── feedback.rs
│   ├── help.rs
│   ├── overlay.rs
│   ├── teams.rs
│   ├── shapes.rs
│   ├── teamsim.rs
│   ├── ui_id.rs
│   ├── ask.rs
│   ├── plan.rs
│   ├── new_mission.rs
│   ├── document.rs
│   ├── status.rs
│   ├── mission.rs
│   ├── wbs.rs
│   ├── zoom.rs
│   ├── tasksrc.rs
│   ├── catalog.rs
│   ├── run_picker.rs
│   ├── db/
│   │   ├── form.rs
│   │   ├── mod.rs
│   │   ├── pending.rs
│   │   ├── sql.rs
│   │   ├── table.rs
│   │   ├── tree.rs
│   │   └── open_file.rs
│   ├── archify/
│   │   ├── animate.rs
│   │   ├── compiled.rs
│   │   ├── focus.rs
│   │   ├── mod.rs
│   │   └── motion.rs
│   ├── about.rs
│   ├── updates.rs
│   ├── onboarding.rs
│   ├── devtools.rs
│   ├── format.rs
│   ├── browser.rs
│   ├── agent_setup.rs
│   └── drone.rs
├── ui/
│   ├── mod.rs
│   ├── chat/
│   │   ├── mod.rs
│   │   └── sidebar.rs
│   ├── kit/
│   │   ├── controls.rs
│   │   ├── menu.rs
│   │   ├── mod.rs
│   │   ├── panel.rs
│   │   ├── canvas.rs
│   │   ├── overlay.rs
│   │   ├── files.rs
│   │   ├── settings.rs
│   │   ├── icons.rs
│   │   ├── ribbon.rs
│   │   ├── popover.rs
│   │   ├── blocks.rs
│   │   ├── colour.rs
│   │   ├── slider.rs
│   │   └── quota.rs
│   ├── editor.rs
│   ├── empty.rs
│   ├── explorer.rs
│   ├── rail.rs
│   ├── shell.rs
│   ├── status_bar.rs
│   ├── terminal.rs
│   ├── titlebar.rs
│   ├── project_menu.rs
│   ├── logs.rs
│   ├── agents/
│   │   ├── mod.rs
│   │   ├── column.rs
│   │   └── sidebar.rs
│   ├── board/
│   │   ├── detail.rs
│   │   ├── mod.rs
│   │   └── form.rs
│   ├── dock/
│   │   ├── mod.rs
│   │   └── skin.rs
│   ├── viewer/
│   │   ├── diagram.rs
│   │   ├── diff.rs
│   │   ├── image.rs
│   │   ├── markdown.rs
│   │   ├── mod.rs
│   │   ├── scene.rs
│   │   ├── viewport.rs
│   │   ├── image_edit.rs
│   │   ├── web.rs
│   │   ├── md_options.rs
│   │   └── zoom_modal.rs
│   ├── sink/
│   │   ├── docs.rs
│   │   ├── mod.rs
│   │   ├── style.rs
│   │   ├── files.rs
│   │   ├── project.rs
│   │   ├── settings.rs
│   │   ├── messages.rs
│   │   ├── a2ui.rs
│   │   ├── script.rs
│   │   ├── teamsim.rs
│   │   └── ext_demo.rs
│   ├── file_picker.rs
│   ├── git/
│   │   ├── changes.rs
│   │   ├── diff.rs
│   │   ├── history.rs
│   │   ├── mod.rs
│   │   └── refs.rs
│   ├── settings.rs
│   ├── work.rs
│   ├── new_pane_menu.rs
│   ├── conversation/
│   │   ├── mod.rs
│   │   └── info.rs
│   ├── languages/
│   │   ├── csharp/
│   │   │   └── highlights.scm
│   │   ├── swift/
│   │   │   └── highlights.scm
│   │   └── markdown/
│   │       └── tags.scm
│   ├── search.rs
│   ├── file_dialog.rs
│   ├── ribbon.rs
│   ├── navigator.rs
│   ├── clone.rs
│   ├── outline.rs
│   ├── stats.rs
│   ├── remote_connect.rs
│   ├── notifications.rs
│   ├── new_agent.rs
│   ├── a2ui.rs
│   ├── overflow_menu.rs
│   ├── tab_menu.rs
│   ├── remote_hosts.rs
│   ├── all_projects.rs
│   ├── tools.rs
│   ├── web_view.rs
│   ├── a2ui/
│   │   ├── path.rs
│   │   ├── registry.rs
│   │   └── svg.rs
│   ├── kb/
│   │   ├── mod.rs
│   │   ├── source_form.rs
│   │   └── password.rs
│   ├── help/
│   │   ├── mod.rs
│   │   └── unavailable.md
│   ├── teams/
│   │   ├── graph.rs
│   │   ├── mod.rs
│   │   ├── status.rs
│   │   └── tasks.rs
│   ├── acp_capabilities.rs
│   ├── feedback.rs
│   ├── new_project_menu.rs
│   ├── run_tool_menu.rs
│   ├── mark.rs
│   ├── size.rs
│   ├── themes.rs
│   ├── help_target.rs
│   ├── ident.rs
│   ├── project_face.rs
│   ├── ask.rs
│   ├── new_mission.rs
│   ├── document.rs
│   ├── mission/
│   │   ├── full.rs
│   │   ├── menu.rs
│   │   ├── mod.rs
│   │   ├── panel.rs
│   │   ├── settings.rs
│   │   ├── wbs.rs
│   │   └── note.rs
│   ├── tasksrc.rs
│   ├── hidden_agents_menu.rs
│   ├── menus.rs
│   ├── catalog.rs
│   ├── db/
│   │   ├── cell_input.rs
│   │   ├── conn_form.rs
│   │   ├── explorer.rs
│   │   ├── grid.rs
│   │   ├── json.rs
│   │   ├── keys.rs
│   │   ├── mod.rs
│   │   ├── plan.rs
│   │   ├── results.rs
│   │   ├── settings.rs
│   │   ├── sql.rs
│   │   └── table.rs
│   ├── mdview/
│   │   ├── blockedit.rs
│   │   ├── blocks.rs
│   │   ├── events.rs
│   │   ├── fences.rs
│   │   ├── inline.rs
│   │   ├── minimap.rs
│   │   ├── mod.rs
│   │   ├── outline.rs
│   │   ├── prose.rs
│   │   ├── search.rs
│   │   ├── structure.rs
│   │   ├── sync.rs
│   │   ├── view.rs
│   │   ├── annotation.rs
│   │   ├── highlight.rs
│   │   ├── selection.rs
│   │   └── zoom.rs
│   ├── archify/
│   │   ├── agent.rs
│   │   ├── finder.rs
│   │   ├── lens.rs
│   │   ├── mod.rs
│   │   ├── paint.rs
│   │   ├── panels.rs
│   │   ├── settings.rs
│   │   ├── theme.rs
│   │   └── viewer.rs
│   ├── about.rs
│   ├── onboarding/
│   │   ├── flows.rs
│   │   ├── mod.rs
│   │   └── agent_setup.rs
│   ├── devtools.rs
│   ├── browser.rs
│   ├── browser_settings.rs
│   ├── tunnels.rs
│   ├── drone_modes.rs
│   └── trust.rs
├── lib.rs
├── theme.rs
├── web_export/
│   ├── assets.rs
│   ├── mod.rs
│   ├── routes.rs
│   ├── server.rs
│   ├── bridge.rs
│   └── archive.rs
├── app/
│   ├── agents.rs      agents, conversations and the new-agent menu
│   ├── board.rs       the task board and the jumps out of it
│   ├── boot.rs        `for_project` — the constructor that opens the bus and builds a window
│   ├── chat.rs        chat, diagrams and viewports
│   ├── editor.rs      open files, tabs and saving
│   ├── explorer.rs    explorer rows, menus and file opening
│   ├── git.rs         the git view and project search
│   ├── mod.rs         `AppState`, `OpenProject`, `PaneState`, `BusHub`, the free window functions and the key bindings
│   ├── panels.rs      the dock: settling mode, layout, visibility and placement
│   ├── picker.rs      the file picker and the log panel
│   ├── projects.rs    add, edit and close a project; preference persistence
│   ├── settings.rs    the settings overlay and harness login
│   ├── shell.rs       project lifecycle, chrome and menus, rail mode, font and zoom, the `Render` impl
│   ├── sink.rs        the kitchen sink's setters
│   ├── wire.rs        `receive()`, the pane and terminal calls, focus drains
│   ├── vim.rs
│   ├── nav.rs
│   ├── clone.rs
│   ├── stats.rs
│   ├── capture.rs
│   ├── clipboard.rs
│   ├── host_browse.rs
│   ├── hosts.rs
│   ├── image_edit.rs
│   ├── remote_connect.rs
│   ├── notifications.rs
│   ├── new_agent.rs
│   ├── host_secrets.rs
│   ├── remote_hosts.rs
│   ├── web_panel.rs
│   ├── ssh_connect.rs
│   ├── kb.rs
│   ├── feedback.rs
│   ├── help.rs
│   ├── teams.rs
│   ├── mark.rs
│   ├── size.rs
│   ├── themes.rs
│   ├── teams_span.rs
│   ├── ask.rs
│   ├── plan.rs
│   ├── new_mission.rs
│   ├── mission.rs
│   ├── tasksrc.rs
│   ├── catalog.rs
│   ├── db/
│   │   ├── explorer.rs
│   │   ├── mod.rs
│   │   ├── settings.rs
│   │   ├── sql.rs
│   │   └── table.rs
│   ├── unix_connect.rs
│   ├── about.rs
│   ├── updates.rs
│   ├── help_images.rs
│   ├── onboarding.rs
│   ├── devtools.rs
│   ├── browser.rs
│   ├── remote_tools.rs
│   ├── tunnels.rs
│   ├── agent_setup.rs
│   └── drone_modes.rs
├── version.rs
├── ext/
│   ├── id.rs
│   ├── ids.rs
│   ├── mod.rs
│   ├── registry.rs
│   ├── settings.rs
│   ├── rail.rs
│   ├── menu.rs
│   ├── runner.rs
│   ├── viewer.rs
│   ├── origin.rs
│   └── drone.rs
├── devtools/
│   ├── codec.rs
│   ├── detect.rs
│   ├── diff.rs
│   ├── hash.rs
│   ├── jwt.rs
│   ├── lorem.rs
│   ├── mod.rs
│   ├── time.rs
│   └── uuids.rs
└── .DS_Store

crates/kubiq-app/src/
├── main.rs            three lines: `run(Boot::default())`
├── lib.rs             `Stores`, `Boot` and `run(boot)` — the whole boot sequence, and the only crate that names both halves
└── handoff.rs
```

<!-- generated:end tree -->

<!-- generated:begin anchors -->

## File to document

Every file a document names in its `code_anchors` frontmatter, inverted: change this file, and check
the documents in its row.

| File | Documents |
|---|---|
| `.claude/excalidraw.py` | [`diagram-format.md`](./diagram-format.md) |
| `.github/workflows/create-release.yml` | [`operations.md`](./operations.md) |
| `.github/workflows/release-linux.yml` | [`operations.md`](./operations.md) |
| `.github/workflows/release-macos.yml` | [`operations.md`](./operations.md) |
| `.github/workflows/release-windows.yml` | [`operations.md`](./operations.md) |
| `Cargo.toml` | [`project-structure.md`](./project-structure.md) |
| `Justfile` | [`operations.md`](./operations.md) |
| `_devops/scripts/bundle-version.sh` | [`operations.md`](./operations.md) |
| `_devops/scripts/channel-manifest.py` | [`operations.md`](./operations.md) |
| `_devops/windows/kubiq.iss` | [`operations.md`](./operations.md) |
| `_tools/Info.plist` | [`operations.md`](./operations.md) |
| `_tools/docs.py` | [`operations.md`](./operations.md) |
| `_tools/drone.py` | [`features/drone.md`](../features/drone.md), [`operations.md`](./operations.md) |
| `_tools/dump.py` | [`operations.md`](./operations.md) |
| `_tools/helpbundle.py` | [`help/coverage.md`](../help/coverage.md), [`help/images.md`](../help/images.md), [`help/structure.md`](../help/structure.md), [`help/style.md`](../help/style.md), [`operations.md`](./operations.md), [`wip/help.md`](../wip/help.md) |
| `_tools/icns.py` | [`operations.md`](./operations.md), [`project-structure.md`](./project-structure.md) |
| `_tools/icons.py` | [`help/images.md`](../help/images.md), [`ui-and-design.md`](./ui-and-design.md) |
| `_tools/licenses.py` | [`operations.md`](./operations.md) |
| `_tools/teamsim/FORMAT.md` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `_tools/teamsim/algos.py` | [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `_tools/teamsim/shapes.py` | [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `_tools/webassets.py` | [`operations.md`](./operations.md) |
| `assets/icons/icons.yaml` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/agent-manager/examples/confined_shell_probe.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/account.rs` | [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/credentials/mod.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/harness/claude.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/harness/mod.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/harness/registry.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/io/acp.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/io/acp_caps.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/io/acp_client.rs` | [`inbox/hitl-registered-dialogs.md`](../inbox/hitl-registered-dialogs.md), [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/io/jsonl.rs` | [`inbox/hitl-registered-dialogs.md`](../inbox/hitl-registered-dialogs.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/agent-manager/src/io/mod.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/io/model.rs` | [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/agent-manager/src/io/structured.rs` | [`agent-manager.md`](./agent-manager.md), [`operations.md`](./operations.md) |
| `crates/agent-manager/src/isolate.rs` | [`agent-manager.md`](./agent-manager.md), [`operations.md`](./operations.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/lib.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/main.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/overlay.rs` | [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/profile.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/agent-manager/src/provision.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/quota.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/agent-manager/src/resolve.rs` | [`agent-manager.md`](./agent-manager.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/agent-manager/src/run.rs` | [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/agent-manager/src/session.rs` | [`features/sessions-and-workspaces.md`](../features/sessions-and-workspaces.md), [`agent-manager.md`](./agent-manager.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/agent-manager/src/spec.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/kubiq-app/Cargo.toml` | [`project-structure.md`](./project-structure.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq-app/build.rs` | [`operations.md`](./operations.md) |
| `crates/kubiq-app/res/kubiq-app.rc` | [`operations.md`](./operations.md) |
| `crates/kubiq-app/src/handoff.rs` | [`operations.md`](./operations.md) |
| `crates/kubiq-app/src/lib.rs` | [`features/drone.md`](../features/drone.md), [`features/logs.md`](../features/logs.md), [`agent-manager.md`](./agent-manager.md), [`architecture.md`](./architecture.md), [`operations.md`](./operations.md) |
| `crates/kubiq-app/src/main.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq-db/src/dbml.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-db/src/driver/mod.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-db/src/driver/structure.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-db/src/edit.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-db/src/lib.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-db/src/sql/readonly.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-draw/src/drawio.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq-draw/src/excalidraw.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq-draw/src/model.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq-drone/Cargo.toml` | [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-drone/src/held.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/lib.rs` | [`features/drone.md`](../features/drone.md), [`architecture.md`](./architecture.md), [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-drone/src/linger.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/main.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/relay.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/scrollback.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/search.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-drone/src/socket.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/state.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-drone/src/tunnel.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/Cargo.toml` | [`agent-manager.md`](./agent-manager.md), [`architecture.md`](./architecture.md), [`operations.md`](./operations.md), [`project-structure.md`](./project-structure.md), [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-host/src/agent.rs` | [`features/sessions-and-workspaces.md`](../features/sessions-and-workspaces.md), [`agent-manager.md`](./agent-manager.md), [`wip/agent-login-note.md`](../wip/agent-login-note.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/kubiq-host/src/armed.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/ask.rs` | [`inbox/hitl-registered-dialogs.md`](../inbox/hitl-registered-dialogs.md), [`agent-manager.md`](./agent-manager.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/assist/api.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/assist/mod.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/assist/providers.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/assist/stub.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/assist/subject.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/browser.rs` | [`architecture.md`](./architecture.md), [`project-structure.md`](./project-structure.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/carrier.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/catalog.rs` | [`agent-manager.md`](./agent-manager.md) |
| `crates/kubiq-host/src/cli_shortcut.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq-host/src/connectors/app.rs` | [`features/connectors.md`](../features/connectors.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-host/src/connectors/flow.rs` | [`features/connectors.md`](../features/connectors.md) |
| `crates/kubiq-host/src/connectors/mod.rs` | [`features/connectors.md`](../features/connectors.md) |
| `crates/kubiq-host/src/connectors/providers.rs` | [`features/connectors.md`](../features/connectors.md) |
| `crates/kubiq-host/src/connectors/store.rs` | [`features/connectors.md`](../features/connectors.md), [`wip/db-explorer.md`](../wip/db-explorer.md) |
| `crates/kubiq-host/src/connectors/tls.rs` | [`features/connectors.md`](../features/connectors.md) |
| `crates/kubiq-host/src/conversation.rs` | [`features/sessions-and-workspaces.md`](../features/sessions-and-workspaces.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`agent-manager.md`](./agent-manager.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/conversation_record.rs` | [`features/sessions-and-workspaces.md`](../features/sessions-and-workspaces.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/coordinator.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/sessions-and-workspaces.md`](../features/sessions-and-workspaces.md), [`features/stats.md`](../features/stats.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`agent-manager.md`](./agent-manager.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md), [`wip/indexing.md`](../wip/indexing.md), [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-host/src/db/agent.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/editors.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/jobs.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/mod.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/secrets.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/session.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/store.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/db/tunnel.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq-host/src/drone_modes.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/environment.rs` | [`agent-manager.md`](./agent-manager.md), [`operations.md`](./operations.md) |
| `crates/kubiq-host/src/feedback/api.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/feedback/issues.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/feedback/keychain.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/feedback/mod.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/files/diff.rs` | [`architecture.md`](./architecture.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/files/ignored.rs` | [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/files/mod.rs` | [`architecture.md`](./architecture.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq-host/src/git/graph.rs` | [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/git/history.rs` | [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/git/mod.rs` | [`architecture.md`](./architecture.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/git/nested.rs` | [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/git/observe.rs` | [`architecture.md`](./architecture.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/git/write.rs` | [`version-control.md`](./version-control.md) |
| `crates/kubiq-host/src/index/ceiling.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/index/mod.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/index/text.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/kb/mod.rs` | [`architecture.md`](./architecture.md), [`project-structure.md`](./project-structure.md), [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/kb/ops.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/kb/store.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/kb/sync.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/kb/vault.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/lib.rs` | [`architecture.md`](./architecture.md), [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-host/src/links.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/mcp/ask.rs` | [`inbox/hitl-registered-dialogs.md`](../inbox/hitl-registered-dialogs.md), [`agent-manager.md`](./agent-manager.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/browser.rs` | [`architecture.md`](./architecture.md), [`project-structure.md`](./project-structure.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/catalogue.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md), [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-host/src/mcp/help.rs` | [`wip/help.md`](../wip/help.md) |
| `crates/kubiq-host/src/mcp/kb.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-host/src/mcp/mission.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/mod.rs` | [`agent-manager.md`](./agent-manager.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/mcp/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/registry.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md), [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-host/src/mcp/remote.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/server.rs` | [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-host/src/mcp/tasks.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mcp/tools.rs` | [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-host/src/mission/mod.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/mission/prompts.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq-host/src/mission/runner.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/notifications/mod.rs` | [`features/notifications.md`](../features/notifications.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/notifications/os.rs` | [`features/notifications.md`](../features/notifications.md) |
| `crates/kubiq-host/src/plan/blocks.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq-host/src/plan/mod.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md), [`wip/planning-system.md`](../wip/planning-system.md) |
| `crates/kubiq-host/src/plan/queue.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq-host/src/plan/service.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`wip/planning-system.md`](../wip/planning-system.md) |
| `crates/kubiq-host/src/projects.rs` | [`features/stats.md`](../features/stats.md), [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md), [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-host/src/pty/mod.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `crates/kubiq-host/src/quota.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/remote.rs` | [`features/drone.md`](../features/drone.md), [`architecture.md`](./architecture.md), [`operations.md`](./operations.md) |
| `crates/kubiq-host/src/repos/clone.rs` | [`features/workbench.md`](../features/workbench.md), [`version-control.md`](./version-control.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-host/src/repos/list.rs` | [`features/workbench.md`](../features/workbench.md), [`version-control.md`](./version-control.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-host/src/repos/mod.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md), [`version-control.md`](./version-control.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-host/src/runners/mod.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `crates/kubiq-host/src/search/hits.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/search/walk.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/search/worker.rs` | [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/settings.rs` | [`features/connectors.md`](../features/connectors.md), [`features/drone.md`](../features/drone.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/shell_integration.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq-host/src/shells.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq-host/src/store/file.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/store/harness.rs` | [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq-host/src/store/memory.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/store/mod.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq-host/src/store/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md), [`wip/planning-system.md`](../wip/planning-system.md) |
| `crates/kubiq-host/src/store/project_dir.rs` | [`project-structure.md`](./project-structure.md), [`wip/db-explorer.md`](../wip/db-explorer.md) |
| `crates/kubiq-host/src/store/usage.rs` | [`features/stats.md`](../features/stats.md), [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-host/src/tasksrc/mod.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/tasksrc/outbound.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/tasksrc/service.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/tasksrc/store.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/tasksrc/sync.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/tasksrc/trello.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq-host/src/trust.rs` | [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-host/src/updates/apply.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/updates/feed.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/updates/mod.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/src/watch/mod.rs` | [`architecture.md`](./architecture.md), [`version-control.md`](./version-control.md), [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-host/src/web_assets/archive.rs` | [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-host/src/web_assets/manifest.rs` | [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-host/src/web_assets/mod.rs` | [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md), [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-host/src/work/mod.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-host/tests/coordinator.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `crates/kubiq-host/tests/files.rs` | [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq-host/tests/usage.rs` | [`features/stats.md`](../features/stats.md) |
| `crates/kubiq-proto/Cargo.toml` | [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-proto/src/ask.rs` | [`features/chat.md`](../features/chat.md), [`agent-manager.md`](./agent-manager.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/assist.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/blocks.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq-proto/src/browser.rs` | [`project-structure.md`](./project-structure.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/bus.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/carrier.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md), [`wip/drone.md`](../wip/drone.md) |
| `crates/kubiq-proto/src/catalog.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/connectors.rs` | [`features/connectors.md`](../features/connectors.md), [`transport-contract.md`](./transport-contract.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-proto/src/conversation.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/db.rs` | [`features/workbench-db.md`](../features/workbench-db.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/drone/manifest.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-proto/src/drone/mod.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-proto/src/feedback.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/files.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/git.rs` | [`transport-contract.md`](./transport-contract.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq-proto/src/ids.rs` | [`features/connectors.md`](../features/connectors.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/kb.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq-proto/src/lib.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq-proto/src/log.rs` | [`features/logs.md`](../features/logs.md), [`architecture.md`](./architecture.md), [`wip/claude-auth-problem.md`](../wip/claude-auth-problem.md) |
| `crates/kubiq-proto/src/mcp.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/messages.rs` | [`features/connectors.md`](../features/connectors.md), [`features/stats.md`](../features/stats.md), [`transport-contract.md`](./transport-contract.md), [`wip/db-explorer.md`](../wip/db-explorer.md), [`wip/drone.md`](../wip/drone.md), [`wip/kb.md`](../wip/kb.md), [`wip/web-panel-phase3.md`](../wip/web-panel-phase3.md) |
| `crates/kubiq-proto/src/mission.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/notifications.rs` | [`features/notifications.md`](../features/notifications.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`transport-contract.md`](./transport-contract.md), [`wip/planning-system.md`](../wip/planning-system.md) |
| `crates/kubiq-proto/src/projects.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md), [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-proto/src/quota.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/remote_tools.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/repos.rs` | [`features/workbench.md`](../features/workbench.md), [`transport-contract.md`](./transport-contract.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq-proto/src/settings.rs` | [`features/connectors.md`](../features/connectors.md), [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md), [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq-proto/src/ssh.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq-proto/src/stats.rs` | [`features/stats.md`](../features/stats.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/tasksrc.rs` | [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/tools.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/tunnel.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/update.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/wire.rs` | [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-proto/src/work.rs` | [`transport-contract.md`](./transport-contract.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/kubiq-tunnel/Cargo.toml` | [`project-structure.md`](./project-structure.md) |
| `crates/kubiq-tunnel/src/duplex.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-tunnel/src/mux.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-tunnel/src/pin.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-tunnel/src/registry.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq-tunnel/src/tls.rs` | [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq/Cargo.toml` | [`project-structure.md`](./project-structure.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/assets/web/bridge.js` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md) |
| `crates/kubiq/assets/web/drawio/app.js` | [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/assets/web/drawio/index.html` | [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/assets/web/excalidraw/app.js` | [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/assets/web/excalidraw/index.html` | [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/build.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md) |
| `crates/kubiq/src/app/about.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/agent_setup.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/src/app/agents.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench-agents.md`](../features/workbench-agents.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq/src/app/ask.rs` | [`features/chat.md`](../features/chat.md), [`features/notifications.md`](../features/notifications.md) |
| `crates/kubiq/src/app/board.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/app/boot.rs` | [`features/chat.md`](../features/chat.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/app/browser.rs` | [`features/workbench.md`](../features/workbench.md), [`project-structure.md`](./project-structure.md) |
| `crates/kubiq/src/app/capture.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/app/catalog.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/chat.rs` | [`features/chat.md`](../features/chat.md) |
| `crates/kubiq/src/app/clipboard.rs` | [`features/chat.md`](../features/chat.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/app/clone.rs` | [`features/workbench.md`](../features/workbench.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq/src/app/db/mod.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/app/db/sql.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/app/db/table.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/app/devtools.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/editor.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/app/explorer.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/app/feedback.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/app/git.rs` | [`features/workbench-git.md`](../features/workbench-git.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq/src/app/help.rs` | [`wip/help.md`](../wip/help.md) |
| `crates/kubiq/src/app/help_images.rs` | [`wip/help.md`](../wip/help.md) |
| `crates/kubiq/src/app/host_browse.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md) |
| `crates/kubiq/src/app/hosts.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/app/image_edit.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/app/kb.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/app/mark.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/app/mission.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/app/mod.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/app/nav.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/new_agent.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/app/new_mission.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/app/notifications.rs` | [`features/notifications.md`](../features/notifications.md) |
| `crates/kubiq/src/app/onboarding.rs` | [`features/onboarding.md`](../features/onboarding.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/panels.rs` | [`features/chat.md`](../features/chat.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/picker.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md) |
| `crates/kubiq/src/app/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/app/projects.rs` | [`features/chat.md`](../features/chat.md), [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/remote_connect.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq/src/app/settings.rs` | [`features/connectors.md`](../features/connectors.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/shell.rs` | [`features/chat.md`](../features/chat.md), [`features/stats.md`](../features/stats.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/app/sink.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/app/size.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/app/ssh_connect.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/app/stats.rs` | [`features/stats.md`](../features/stats.md) |
| `crates/kubiq/src/app/tasksrc.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq/src/app/teams.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`components.md`](./components.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/app/teams_span.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md) |
| `crates/kubiq/src/app/tunnels.rs` | [`features/drone.md`](../features/drone.md), [`transport-contract.md`](./transport-contract.md) |
| `crates/kubiq/src/app/unix_connect.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/updates.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/vim.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/app/web_panel.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/kb.md`](../wip/kb.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/app/wire.rs` | [`features/chat.md`](../features/chat.md), [`features/notifications.md`](../features/notifications.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/stats.md`](../features/stats.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`architecture.md`](./architecture.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/devtools/codec.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/detect.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/diff.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/hash.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/jwt.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/lorem.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/mod.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/time.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/devtools/uuids.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ext/id.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/ids.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/menu.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/mod.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/origin.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq/src/ext/rail.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/registry.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/runner.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/settings.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/ext/viewer.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/lib.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/state/a2ui.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/action.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/eval.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/kubiq-catalog.json` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/live.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/path.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/svg.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/a2ui/value.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/about.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/agent_setup.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/src/state/agents.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md), [`components.md`](./components.md) |
| `crates/kubiq/src/state/ask.rs` | [`features/chat.md`](../features/chat.md), [`inbox/hitl-registered-dialogs.md`](../inbox/hitl-registered-dialogs.md) |
| `crates/kubiq/src/state/board.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/state/browser.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/catalog.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/chat.rs` | [`features/chat.md`](../features/chat.md) |
| `crates/kubiq/src/state/clone.rs` | [`features/workbench.md`](../features/workbench.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq/src/state/conversation.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench-agents.md`](../features/workbench-agents.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`components.md`](./components.md), [`wip/agent-setup.md`](../wip/agent-setup.md) |
| `crates/kubiq/src/state/db/mod.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/db/open_file.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/db/pending.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/db/sql.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/db/table.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/db/tree.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/state/devtools.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/diagrams.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/state/dock.rs` | [`features/chat.md`](../features/chat.md), [`features/logs.md`](../features/logs.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md), [`wip/db-explorer.md`](../wip/db-explorer.md), [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/state/document.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`wip/planning-system.md`](../wip/planning-system.md) |
| `crates/kubiq/src/state/editor.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md), [`wip/kb.md`](../wip/kb.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/state/explorer/menu.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/state/explorer/mod.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/state/explorer/rows.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/explorer/tree.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/feedback.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/file_picker.rs` | [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/state/git.rs` | [`features/workbench-git.md`](../features/workbench-git.md), [`version-control.md`](./version-control.md) |
| `crates/kubiq/src/state/help.rs` | [`wip/help.md`](../wip/help.md) |
| `crates/kubiq/src/state/image_edit.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/kb.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/state/layout.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/state/logs.rs` | [`features/logs.md`](../features/logs.md) |
| `crates/kubiq/src/state/mission.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/state/mod.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/nav.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/nav/text.rs` | [`features/workbench.md`](../features/workbench.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/state/navigator.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/new_agent.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/state/new_mission.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/state/notifications.rs` | [`features/notifications.md`](../features/notifications.md) |
| `crates/kubiq/src/state/onboarding.rs` | [`features/onboarding.md`](../features/onboarding.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/overlay.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/state/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/state/prefs.rs` | [`features/chat.md`](../features/chat.md), [`features/onboarding.md`](../features/onboarding.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/state/remote.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/state/run_picker.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/scene.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/script.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/settings.rs` | [`features/connectors.md`](../features/connectors.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/shapes.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/state/sink.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/state/stats.rs` | [`features/stats.md`](../features/stats.md) |
| `crates/kubiq/src/state/status.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/state/tasksrc.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq/src/state/teams.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/state/teamsim.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/state/ui_id.rs` | [`help/coverage.md`](../help/coverage.md), [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/src/state/updates.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/viewport.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/state/vim/mod.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/vim/motion.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/vim/object.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/vim/search.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/vim/step.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/wbs.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/state/web_panel.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/src/state/when.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/windows.rs` | [`features/workbench.md`](../features/workbench.md), [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/state/work.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/state/workbench.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench-agents.md`](../features/workbench-agents.md), [`features/workbench.md`](../features/workbench.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/theme.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/a2ui.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/a2ui/path.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/a2ui/registry.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/a2ui/svg.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/about.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/acp_capabilities.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md) |
| `crates/kubiq/src/ui/agents/column.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq/src/ui/agents/mod.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/ui/agents/sidebar.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/ui/all_projects.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/ask.rs` | [`features/chat.md`](../features/chat.md) |
| `crates/kubiq/src/ui/board/detail.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/board/form.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/board/mod.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/browser.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/catalog.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/chat/mod.rs` | [`features/chat.md`](../features/chat.md) |
| `crates/kubiq/src/ui/chat/sidebar.rs` | [`features/chat.md`](../features/chat.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq/src/ui/clone.rs` | [`features/workbench.md`](../features/workbench.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md) |
| `crates/kubiq/src/ui/conversation/info.rs` | [`features/chat.md`](../features/chat.md) |
| `crates/kubiq/src/ui/conversation/mod.rs` | [`features/chat.md`](../features/chat.md), [`features/workbench-agents.md`](../features/workbench-agents.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/agent-setup.md`](../wip/agent-setup.md), [`wip/agent-vocabulary.md`](../wip/agent-vocabulary.md) |
| `crates/kubiq/src/ui/db/conn_form.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/db/explorer.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/db/mod.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/db/settings.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/db/sql.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/db/table.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/src/ui/devtools.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/dock/mod.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/stats.md`](../features/stats.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/dock/skin.rs` | [`features/chat.md`](../features/chat.md), [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/document.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/editor.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md) |
| `crates/kubiq/src/ui/empty.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/explorer.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/feedback.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/file_dialog.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md), [`wip/kb.md`](../wip/kb.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/file_picker.rs` | [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/git/changes.rs` | [`features/workbench-git.md`](../features/workbench-git.md) |
| `crates/kubiq/src/ui/git/diff.rs` | [`features/workbench-git.md`](../features/workbench-git.md), [`components.md`](./components.md) |
| `crates/kubiq/src/ui/git/history.rs` | [`features/workbench-git.md`](../features/workbench-git.md) |
| `crates/kubiq/src/ui/git/mod.rs` | [`features/workbench-git.md`](../features/workbench-git.md) |
| `crates/kubiq/src/ui/git/refs.rs` | [`features/workbench-git.md`](../features/workbench-git.md) |
| `crates/kubiq/src/ui/help/mod.rs` | [`wip/help.md`](../wip/help.md) |
| `crates/kubiq/src/ui/help_target.rs` | [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/src/ui/hidden_agents_menu.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/ident.rs` | [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/src/ui/kb/mod.rs` | [`components.md`](./components.md), [`wip/db-explorer.md`](../wip/db-explorer.md), [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/ui/kb/password.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/ui/kb/source_form.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/ui/kit/blocks.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/ui/kit/canvas.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/colour.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/controls.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/files.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/menu.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`features/workbench-teams.md`](../features/workbench-teams.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/clone-a-project.md`](../wip/clone-a-project.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/ui/kit/mod.rs` | [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/overlay.rs` | [`features/onboarding.md`](../features/onboarding.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/popover.rs` | [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/ribbon.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/kit/settings.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/logs.rs` | [`features/logs.md`](../features/logs.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/mark.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/mdview/annotation.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mdview/blockedit.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/mdview/minimap.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/mdview/outline.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/mdview/search.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/mdview/view.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/menus.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/mission/full.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/menu.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/mod.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/note.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/panel.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/settings.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mission/wbs.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/mod.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/navigator.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/new_agent.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/src/ui/new_mission.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/src/ui/new_pane_menu.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `crates/kubiq/src/ui/new_project_menu.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/notifications.rs` | [`features/notifications.md`](../features/notifications.md) |
| `crates/kubiq/src/ui/onboarding/agent_setup.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/src/ui/onboarding/flows.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/src/ui/onboarding/mod.rs` | [`features/onboarding.md`](../features/onboarding.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/outline.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/overflow_menu.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/project_face.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/project_menu.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/rail.rs` | [`features/workbench.md`](../features/workbench.md), [`wip/db-explorer.md`](../wip/db-explorer.md), [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/src/ui/remote_connect.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/remote_hosts.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/ribbon.rs` | [`features/workbench-git.md`](../features/workbench-git.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/run_tool_menu.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/search.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/settings.rs` | [`features/connectors.md`](../features/connectors.md), [`features/drone.md`](../features/drone.md), [`features/workbench.md`](../features/workbench.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/indexing.md`](../wip/indexing.md) |
| `crates/kubiq/src/ui/shell.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/sink/a2ui.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/docs.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/ext_demo.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/sink/files.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/mod.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/project.rs` | [`features/drone.md`](../features/drone.md), [`features/workbench-sink.md`](../features/workbench-sink.md), [`features/workbench.md`](../features/workbench.md), [`wip/db-explorer.md`](../wip/db-explorer.md), [`wip/indexing.md`](../wip/indexing.md), [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/src/ui/sink/script.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/settings.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/src/ui/sink/style.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/sink/teamsim.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md), [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/ui/size.rs` | [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/stats.rs` | [`features/stats.md`](../features/stats.md) |
| `crates/kubiq/src/ui/status_bar.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/tab_menu.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/tasksrc.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq/src/ui/teams/graph.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`inbox/subagent-status-dictionaries-and-hexagon.md`](../inbox/subagent-status-dictionaries-and-hexagon.md), [`components.md`](./components.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md), [`wip/teams-layout-spike.md`](../wip/teams-layout-spike.md) |
| `crates/kubiq/src/ui/teams/mod.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/src/ui/teams/status.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md), [`features/workbench-teams.md`](../features/workbench-teams.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/teams/tasks.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md) |
| `crates/kubiq/src/ui/terminal.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/titlebar.rs` | [`features/notifications.md`](../features/notifications.md), [`features/workbench.md`](../features/workbench.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/src/ui/tools.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md), [`components.md`](./components.md) |
| `crates/kubiq/src/ui/trust.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/src/ui/tunnels.rs` | [`features/drone.md`](../features/drone.md) |
| `crates/kubiq/src/ui/viewer/diagram.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/viewer/diff.rs` | [`features/workbench-git.md`](../features/workbench-git.md), [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/viewer/image.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/viewer/image_edit.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/viewer/markdown.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/viewer/md_options.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/viewer/mod.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`components.md`](./components.md), [`ui-and-design.md`](./ui-and-design.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/viewer/scene.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/ui/viewer/viewport.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/src/ui/viewer/web.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/web_view.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md), [`features/workbench.md`](../features/workbench.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/ui/work.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md), [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/src/version.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/web_export/archive.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/src/web_export/assets.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/src/web_export/bridge.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md), [`wip/web-panel-phase6.md`](../wip/web-panel-phase6.md) |
| `crates/kubiq/src/web_export/mod.rs` | [`architecture.md`](./architecture.md) |
| `crates/kubiq/src/web_export/routes.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md), [`wip/web-panel-phase45.md`](../wip/web-panel-phase45.md) |
| `crates/kubiq/src/web_export/server.rs` | [`wip/eb-panel-phase6.md`](../wip/eb-panel-phase6.md), [`wip/web-panel-phase2.md`](../wip/web-panel-phase2.md) |
| `crates/kubiq/tests/a2ui.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/tests/agent_setup.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/tests/agents.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/tests/board.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/tests/bookmarks.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/catalog.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/conversation.rs` | [`features/workbench-agents.md`](../features/workbench-agents.md) |
| `crates/kubiq/tests/db_open_file.rs` | [`features/workbench-db.md`](../features/workbench-db.md) |
| `crates/kubiq/tests/diagrams.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/dismiss.rs` | [`ui-and-design.md`](./ui-and-design.md) |
| `crates/kubiq/tests/dock.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/explorer.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/file_picker.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/files_changed.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/git.rs` | [`features/workbench-git.md`](../features/workbench-git.md) |
| `crates/kubiq/tests/image_gestures.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/kb.rs` | [`wip/kb.md`](../wip/kb.md) |
| `crates/kubiq/tests/mode_restore.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/nav.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/nav_text.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/navigator.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/new_project.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/onboarding.rs` | [`features/onboarding.md`](../features/onboarding.md) |
| `crates/kubiq/tests/plan.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md) |
| `crates/kubiq/tests/prefs.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/rail_container.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/scene.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/script.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/tests/settings.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/settings_container.rs` | [`features/workbench.md`](../features/workbench.md) |
| `crates/kubiq/tests/sink.rs` | [`features/workbench-sink.md`](../features/workbench-sink.md) |
| `crates/kubiq/tests/stats.rs` | [`features/stats.md`](../features/stats.md) |
| `crates/kubiq/tests/tasksrc.rs` | [`features/workbench-tasks.md`](../features/workbench-tasks.md), [`inbox/task-sources-proposal.md`](../inbox/task-sources-proposal.md) |
| `crates/kubiq/tests/teams.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md), [`wip/teams-cross-project.md`](../wip/teams-cross-project.md) |
| `crates/kubiq/tests/teams_view.rs` | [`features/workbench-teams.md`](../features/workbench-teams.md) |
| `crates/kubiq/tests/ui_id.rs` | [`wip/in-place-help.md`](../wip/in-place-help.md) |
| `crates/kubiq/tests/viewer_container.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/viewer_kind.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/viewport.rs` | [`features/workbench-ide.md`](../features/workbench-ide.md) |
| `crates/kubiq/tests/vim.rs` | [`features/workbench.md`](../features/workbench.md) |
| `help/manifest.toml` | [`help/coverage.md`](../help/coverage.md), [`help/images.md`](../help/images.md), [`help/structure.md`](../help/structure.md), [`help/style.md`](../help/style.md) |
| `help/onboarding/done.md` | [`features/onboarding.md`](../features/onboarding.md) |
| `help/onboarding/ide-done.md` | [`features/onboarding.md`](../features/onboarding.md) |
| `help/onboarding/ide.md` | [`features/onboarding.md`](../features/onboarding.md) |
| `help/onboarding/tour.md` | [`features/onboarding.md`](../features/onboarding.md) |
| `help/onboarding/welcome.md` | [`features/onboarding.md`](../features/onboarding.md) |
| `vendor/gpui-terminal/Cargo.toml` | [`project-structure.md`](./project-structure.md) |
| `vendor/gpui-terminal/src/clipboard.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/event.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/input.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/mouse.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/render.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/terminal.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |
| `vendor/gpui-terminal/src/view.rs` | [`features/panes-and-terminals.md`](../features/panes-and-terminals.md) |

## Unanchored

No document's `code_anchors` names these. Restricted to Kubiq's own crates.

| File |
|---|
| `crates/kubiq-host/src/acp.rs` |
| `crates/kubiq-host/src/archify.rs` |
| `crates/kubiq-host/src/assist/apple.rs` |
| `crates/kubiq-host/src/atomic.rs` |
| `crates/kubiq-host/src/config.rs` |
| `crates/kubiq-host/src/connectors/http.rs` |
| `crates/kubiq-host/src/db/tests.rs` |
| `crates/kubiq-host/src/drone/mod.rs` |
| `crates/kubiq-host/src/files/browse.rs` |
| `crates/kubiq-host/src/files/path.rs` |
| `crates/kubiq-host/src/files/related.rs` |
| `crates/kubiq-host/src/gc.rs` |
| `crates/kubiq-host/src/health.rs` |
| `crates/kubiq-host/src/help/archive.rs` |
| `crates/kubiq-host/src/help/mod.rs` |
| `crates/kubiq-host/src/host_meta.rs` |
| `crates/kubiq-host/src/host_path.rs` |
| `crates/kubiq-host/src/mcp/archify.rs` |
| `crates/kubiq-host/src/mcp/sql/blobs.rs` |
| `crates/kubiq-host/src/mcp/sql/encode.rs` |
| `crates/kubiq-host/src/mcp/sql/mod.rs` |
| `crates/kubiq-host/src/plan/lineage.rs` |
| `crates/kubiq-host/src/plan/lines.rs` |
| `crates/kubiq-host/src/plan/provenance.rs` |
| `crates/kubiq-host/src/reply.rs` |
| `crates/kubiq-host/src/runners/just.rs` |
| `crates/kubiq-host/src/runners/make.rs` |
| `crates/kubiq-host/src/runners/mise.rs` |
| `crates/kubiq-host/src/search/ceiling.rs` |
| `crates/kubiq-host/src/search/fallback.rs` |
| `crates/kubiq-host/src/search/mod.rs` |
| `crates/kubiq-host/src/store/browser_history.rs` |
| `crates/kubiq-host/src/web_assets/manifest_drawio.rs` |
| `crates/kubiq-host/src/work/mock.rs` |
| `crates/kubiq-proto/src/acp.rs` |
| `crates/kubiq-proto/src/help.rs` |
| `crates/kubiq-proto/src/merge.rs` |
| `crates/kubiq-proto/src/search.rs` |
| `crates/kubiq/src/app/db/explorer.rs` |
| `crates/kubiq/src/app/db/settings.rs` |
| `crates/kubiq/src/app/drone_modes.rs` |
| `crates/kubiq/src/app/host_secrets.rs` |
| `crates/kubiq/src/app/remote_hosts.rs` |
| `crates/kubiq/src/app/remote_tools.rs` |
| `crates/kubiq/src/app/themes.rs` |
| `crates/kubiq/src/ext/drone.rs` |
| `crates/kubiq/src/state/archify/animate.rs` |
| `crates/kubiq/src/state/archify/compiled.rs` |
| `crates/kubiq/src/state/archify/focus.rs` |
| `crates/kubiq/src/state/archify/mod.rs` |
| `crates/kubiq/src/state/archify/motion.rs` |
| `crates/kubiq/src/state/db/form.rs` |
| `crates/kubiq/src/state/drone.rs` |
| `crates/kubiq/src/state/explorer/filter.rs` |
| `crates/kubiq/src/state/explorer/keys.rs` |
| `crates/kubiq/src/state/format.rs` |
| `crates/kubiq/src/state/remote_hosts.rs` |
| `crates/kubiq/src/state/search.rs` |
| `crates/kubiq/src/state/zoom.rs` |
| `crates/kubiq/src/ui/archify/agent.rs` |
| `crates/kubiq/src/ui/archify/finder.rs` |
| `crates/kubiq/src/ui/archify/lens.rs` |
| `crates/kubiq/src/ui/archify/mod.rs` |
| `crates/kubiq/src/ui/archify/paint.rs` |
| `crates/kubiq/src/ui/archify/panels.rs` |
| `crates/kubiq/src/ui/archify/settings.rs` |
| `crates/kubiq/src/ui/archify/theme.rs` |
| `crates/kubiq/src/ui/archify/viewer.rs` |
| `crates/kubiq/src/ui/browser_settings.rs` |
| `crates/kubiq/src/ui/db/cell_input.rs` |
| `crates/kubiq/src/ui/db/grid.rs` |
| `crates/kubiq/src/ui/db/json.rs` |
| `crates/kubiq/src/ui/db/keys.rs` |
| `crates/kubiq/src/ui/db/plan.rs` |
| `crates/kubiq/src/ui/db/results.rs` |
| `crates/kubiq/src/ui/drone_modes.rs` |
| `crates/kubiq/src/ui/kit/icons.rs` |
| `crates/kubiq/src/ui/kit/panel.rs` |
| `crates/kubiq/src/ui/kit/quota.rs` |
| `crates/kubiq/src/ui/kit/slider.rs` |
| `crates/kubiq/src/ui/mdview/blocks.rs` |
| `crates/kubiq/src/ui/mdview/events.rs` |
| `crates/kubiq/src/ui/mdview/fences.rs` |
| `crates/kubiq/src/ui/mdview/highlight.rs` |
| `crates/kubiq/src/ui/mdview/inline.rs` |
| `crates/kubiq/src/ui/mdview/mod.rs` |
| `crates/kubiq/src/ui/mdview/prose.rs` |
| `crates/kubiq/src/ui/mdview/selection.rs` |
| `crates/kubiq/src/ui/mdview/structure.rs` |
| `crates/kubiq/src/ui/mdview/sync.rs` |
| `crates/kubiq/src/ui/mdview/zoom.rs` |
| `crates/kubiq/src/ui/sink/messages.rs` |
| `crates/kubiq/src/ui/themes.rs` |
| `crates/kubiq/src/ui/viewer/zoom_modal.rs` |

<!-- generated:end anchors -->

## Related docs

- [`project-structure.md`](./project-structure.md) — what belongs in each folder, and what never does
- [`architecture.md`](./architecture.md) — why the modules divide this way
- `_docs/_meta/authoring.md` — the duty the anchor table exists to serve
