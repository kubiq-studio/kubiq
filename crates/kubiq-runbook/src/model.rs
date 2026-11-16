//! The document model: frontmatter settings and the runnable cells, read from [`crate::format`].
//!
//! A fence is a **cell** when it carries an attribute group and is not an `output` block, or when it
//! is a ```` ```check ```` block. A plain fence with no attributes is an example in prose and runs
//! nothing. `output` blocks are **host-owned**: they are projected from the evidence
//! ([`crate::project`]) and never parsed back as facts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::format::{self, Attrs, BlockKind, Source};

/// The fence language of a host-owned block.
pub const OUTPUT: &str = "output";
/// The fence language of a manual check.
pub const CHECK: &str = "check";

/// What a cell is in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellKind {
    /// An operation: a vertex of the step graph.
    Step,
    /// An automatic check of a step (`check-of=`): a script whose exit 0 (or `expect=` match) is a
    /// pass.
    Check,
    /// A manual check (```` ```check {… manual} ````): a checklist a human marks pass or fail.
    Manual,
}

/// One declared parameter. The base reads the declaration so it can show it and lint it; binding a
/// value is a provider's (`crate::service::Provider::bindings`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ParamSpec {
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// A reference to a secret, never the secret (`vault:kv/db/{{env}}/admin`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
}

/// The frontmatter fields the engine reads. Every other key is kept in the file untouched.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Front {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `every-cell` (the default) or `readonly-auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// The YAML did not parse: the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `retry=NxT`: up to `times` tries, `every_secs` apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retry {
    pub times: u32,
    pub every_secs: u64,
}

/// One runnable cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// The explicit `#id`, or `@<line>` when the fence names none (a lint).
    pub id: String,
    pub explicit: bool,
    /// The fence language (`sh`, `bash`, `check`).
    pub lang: String,
    pub kind: CellKind,
    /// `needs=` as written: `None` when absent (the previous step), `Some(vec![])` for `needs=`.
    pub needs: Option<Vec<String>>,
    pub check_of: Option<String>,
    pub readonly: bool,
    pub confirm: Option<String>,
    pub target: Option<String>,
    pub interactive: bool,
    pub timeout_secs: Option<u64>,
    pub shell: Option<String>,
    pub retry: Option<Retry>,
    pub expect: Option<String>,
    /// The command (or, for a manual check, the checklist).
    pub body: String,
    /// 1-based line of the opening fence.
    pub line: usize,
    /// Index into [`Runbook::source`]'s blocks.
    pub block: usize,
    /// Attribute problems found while reading (bad `timeout=`, bad `retry=`).
    pub problems: Vec<String>,
}

impl Cell {
    pub fn is_step(&self) -> bool {
        self.kind == CellKind::Step
    }
}

/// A parsed runbook.
#[derive(Debug, Clone, PartialEq)]
pub struct Runbook {
    pub source: Source,
    pub front: Front,
    pub cells: Vec<Cell>,
}

/// `30s`, `5m`, `2h`, or bare seconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let text = text.trim();
    let (num, mult) = match text.chars().last()? {
        's' => (&text[..text.len() - 1], 1),
        'm' => (&text[..text.len() - 1], 60),
        'h' => (&text[..text.len() - 1], 3600),
        _ => (text, 1),
    };
    num.parse::<u64>().ok().map(|n| n * mult)
}

fn parse_retry(text: &str) -> Option<Retry> {
    let (times, every) = text.split_once('x')?;
    Some(Retry {
        times: times.parse().ok().filter(|n| *n > 0)?,
        every_secs: parse_duration(every)?,
    })
}

fn cell_of(attrs: &Attrs, lang: &str, body: &str, line: usize, block: usize) -> Cell {
    let mut problems = Vec::new();
    let kind = if lang == CHECK {
        CellKind::Manual
    } else if attrs.get("check-of").is_some() {
        CellKind::Check
    } else {
        CellKind::Step
    };
    let needs = attrs.get("needs").map(|v| {
        v.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_start_matches('#').to_string())
            .collect()
    });
    let timeout_secs = attrs.get("timeout").and_then(|t| {
        let parsed = parse_duration(t);
        if parsed.is_none() {
            problems.push(format!("timeout={t} is not a duration (30s, 5m, 2h)"));
        }
        parsed
    });
    let retry = attrs.get("retry").and_then(|r| {
        let parsed = parse_retry(r);
        if parsed.is_none() {
            problems.push(format!("retry={r} is not NxT (10x15s)"));
        }
        parsed
    });
    if let Some(expect) = attrs.get("expect")
        && regex::Regex::new(expect).is_err()
    {
        problems.push(format!("expect={expect} is not a regular expression"));
    }
    Cell {
        id: attrs.id.clone().unwrap_or_else(|| format!("@{line}")),
        explicit: attrs.id.is_some(),
        lang: lang.to_string(),
        kind,
        needs,
        check_of: attrs
            .get("check-of")
            .map(|s| s.trim_start_matches('#').to_string()),
        readonly: attrs.has("readonly"),
        confirm: attrs.get("confirm").map(str::to_string),
        target: attrs.get("target").map(str::to_string),
        interactive: attrs.has("interactive"),
        timeout_secs,
        shell: attrs.get("shell").map(str::to_string),
        retry,
        expect: attrs.get("expect").map(str::to_string),
        body: body.to_string(),
        line,
        block,
        problems,
    }
}

impl Runbook {
    pub fn parse(text: &str) -> Runbook {
        let source = format::scan(text);
        let front = match &source.front {
            None => Front::default(),
            Some((yaml, _)) if yaml.trim().is_empty() => Front::default(),
            Some((yaml, _)) => match serde_yaml::from_str::<Front>(yaml) {
                Ok(front) => front,
                Err(e) => Front {
                    error: Some(e.to_string()),
                    ..Front::default()
                },
            },
        };
        let mut cells = Vec::new();
        for (index, block) in source.blocks.iter().enumerate() {
            let BlockKind::Fence(fence) = &block.kind else {
                continue;
            };
            if fence.lang == OUTPUT {
                continue;
            }
            let attrs = match (&fence.attrs, fence.lang.as_str()) {
                (Some(a), _) => a.clone(),
                (None, CHECK) => Attrs::default(),
                (None, _) => continue,
            };
            cells.push(cell_of(&attrs, &fence.lang, &fence.body, block.line, index));
        }
        Runbook {
            source,
            front,
            cells,
        }
    }

    pub fn cell(&self, id: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    /// The steps, in document order.
    pub fn steps(&self) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(|c| c.is_step())
    }

    /// The checks of `step`, in document order.
    pub fn checks_of<'a>(&'a self, step: &'a str) -> impl Iterator<Item = &'a Cell> + 'a {
        self.cells
            .iter()
            .filter(move |c| !c.is_step() && c.check_of.as_deref() == Some(step))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"---
runbook: rotate-db-credentials
params:
  env: { required: true, choices: [staging, prod] }
  db_host: { default: "db.{{env}}.internal" }
  admin_secret: { secret_ref: "vault:kv/db/{{env}}/admin" }
target: local
---

# Rotate

```sh {#lag readonly timeout=30s}
psql -h {{db_host}} -c "select 1"
```

```sh
echo just an example
```

```sh {#rotate needs=lag confirm="This changes the prod password"}
./scripts/rotate.sh --host {{db_host}}
```

```sh {#rotate-ok check-of=rotate retry=10x15s expect="^1$"}
psql -c "select 1"
```

```check {#app-ok check-of=rotate manual}
- [ ] no auth errors
```
"#;

    #[test]
    fn the_proposal_example_reads_as_four_cells() {
        let book = Runbook::parse(DOC);
        assert_eq!(book.front.runbook.as_deref(), Some("rotate-db-credentials"));
        assert!(book.front.params["env"].required);
        assert_eq!(book.front.params["env"].choices, ["staging", "prod"]);
        assert_eq!(
            book.front.params["admin_secret"].secret_ref.as_deref(),
            Some("vault:kv/db/{{env}}/admin")
        );
        let ids: Vec<_> = book.cells.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["lag", "rotate", "rotate-ok", "app-ok"]);
        let lag = book.cell("lag").unwrap();
        assert!(lag.readonly);
        assert_eq!(lag.timeout_secs, Some(30));
        let ok = book.cell("rotate-ok").unwrap();
        assert_eq!(ok.kind, CellKind::Check);
        assert_eq!(
            ok.retry,
            Some(Retry {
                times: 10,
                every_secs: 15
            })
        );
        assert_eq!(book.cell("app-ok").unwrap().kind, CellKind::Manual);
        assert_eq!(
            book.cell("rotate").unwrap().confirm.as_deref(),
            Some("This changes the prod password")
        );
        assert_eq!(book.checks_of("rotate").count(), 2);
    }

    #[test]
    fn bad_frontmatter_is_reported_not_fatal() {
        let book = Runbook::parse("---\nparams: [\n---\n```sh {#a}\ntrue\n```\n");
        assert!(book.front.error.is_some());
        assert_eq!(book.cells.len(), 1);
    }
}
