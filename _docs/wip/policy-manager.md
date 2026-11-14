# WIP — Policy manager

Working notes for the policy manager task. Deleted when the task closes.

## Goal

Manage the isol8 sandbox policies isolated agents run under:

- **Settings → Policies** section: list every policy (origin badge, default toggle), create, edit
  (local only), clone (any), delete (local/imported), import from a remote git repo.
- A **local policy repository** at `<config root>/isol8/profiles/` (already on isol8's
  `profile_paths` — `isolate::plan`). Kubiq metadata (description, default flag, import sources) in
  `<config root>/isol8/policies.toml`.
- Policies are selectable in the **agent definition** editor and the **New agent** dialog
  (`AgentDefinition::policies`, `StartConversation::policies`, `AgentPicks::policies`).
- **Default** policies are added to every isolated run, on top of the picks.
- The **edit dialog** follows isol8's layer structure (`isol8::Profile`), in isol8's order:
  name + description → requires → filter (os/arch/executables) → paths (path, access, match) →
  env → home replace → rewrite.ensure_args → conditional `[[policies]]` rules → macOS
  (capabilities, raw SBPL) → Windows capabilities. Plus a read-only TOML preview.

## Vocabulary

isol8 calls a policy file a *profile layer*; its name is its path relative to the folder, no
extension (`toolchains/rust`). Built-in layers are embedded in isol8 (74). isol8 has no layer
editor and no layer importer — both are Kubiq's.

## Wire (done — `crates/kubiq-proto/src/policy.rs`, `messages.rs` "Policy family")

`ListPolicies` → `Policies{policies}`; `ReadPolicy{name}` → `PolicyBody{info, doc, toml}`;
`SavePolicy{name, replaces, description, doc}`; `ClonePolicy{name,new_name}`; `DeletePolicy{name}`;
`SetPolicyDefault{name, default}`; `ImportPolicies{url, git_ref}` → `PolicyImported{url, names}`;
failures → `PolicyError{error}`. Every mutation re-broadcasts `Policies` to every client.

## Library (agent-manager) — `src/policy.rs`

- `PolicyStore { state_dir }` (state_dir = `<root>/isol8`): `list`, `read` → `(isol8::Profile, toml)`,
  `save(name, replaces, description, Profile)`, `clone`, `delete`, `set_default`, `defaults()`,
  `import(url, git_ref)`.
- Listing via `isol8::profile::LayerRegistry::load(&[<state_dir>/profiles])` (origins Builtin,
  UserConfig → User, ProfilePath → Local, or Imported when under `profiles/imported/<slug>/`).
- Serialize with `isol8::profile::format_layer`; validate `requires` with
  `isol8::profile::resolve_requires`.
- Import: `git clone --depth 1 [--branch ref] <url>` into a temp dir, take every `*.toml` under
  `profiles/` (else the repo root) that parses as `isol8::Profile`, copy to
  `profiles/imported/<slug>/<rel>`; record the source in `policies.toml`. A re-import replaces the
  folder.
- `isolate::plan` takes a list of extra layers (`IsolateOptions` or `RunSpec`) instead of only the
  single `Isolation::Sandboxed` name; host passes defaults ∪ definition ∪ launch picks, dropping
  names the registry no longer resolves.
- The on-disk definition record (`Profile` in `profile.rs`) gains `policies: Vec<String>`.
