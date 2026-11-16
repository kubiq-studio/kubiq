//! `{{name}}` substitution and the command hash an approval binds.
//!
//! Binding a value to a name is not the engine's: a provider answers it
//! (`crate::service::Provider::bindings`), and with none a runbook binds each declared parameter to
//! its literal `default` and nothing else. What the engine owns is the substitution itself and the
//! rule that a secret's value never reaches a document, a log or a hash: the **display** form shows
//! `•••` and is what is hashed; only the **exec** form, handed to the shell, carries the value.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::Runbook;

/// What a secret shows as wherever a person or an agent can read it.
pub const MASK: &str = "•••";

/// One bound value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub value: String,
    /// A secret: shown as [`MASK`], masked in every capture.
    #[serde(default)]
    pub secret: bool,
}

/// Every bound name of one document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bindings {
    pub values: BTreeMap<String, Binding>,
    /// Required parameters nobody bound, as `name: why`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

impl Bindings {
    /// The base's own bindings: each declared parameter's `default`, taken literally (a default
    /// that names another parameter stays as written — resolving it is a provider's).
    pub fn defaults(book: &Runbook) -> Bindings {
        let mut out = Bindings::default();
        for (name, spec) in &book.front.params {
            if let Some(default) = &spec.default {
                out.values.insert(
                    name.clone(),
                    Binding {
                        value: default.clone(),
                        secret: false,
                    },
                );
            } else if spec.required {
                out.missing.push(format!("{name}: required and not bound"));
            }
        }
        out
    }

    /// The secret values, for the masker.
    pub fn secrets(&self) -> Vec<String> {
        self.values
            .values()
            .filter(|b| b.secret && !b.value.is_empty())
            .map(|b| b.value.clone())
            .collect()
    }
}

/// Which form to substitute into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Secrets as [`MASK`]: shown, hashed, recorded.
    Display,
    /// Secrets as their value: handed to the shell, never stored.
    Exec,
    /// Secrets as `{{secret:<name>}}`: what the approval hash covers, so swapping one secret for
    /// another changes the hash while no value ever reaches it.
    Hash,
}

/// Substitute every `{{ name }}` in `text`. Answers the text and the names that were not bound,
/// which are left in the text as written.
pub fn subst(text: &str, bindings: &Bindings, form: Form) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut unbound = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else {
            break;
        };
        let name = rest[open + 2..open + 2 + close].trim();
        out.push_str(&rest[..open]);
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        match bindings.values.get(name).filter(|_| valid) {
            Some(b) if b.secret && form == Form::Display => out.push_str(MASK),
            Some(b) if b.secret && form == Form::Hash => {
                out.push_str("{{secret:");
                out.push_str(name);
                out.push_str("}}");
            }
            Some(b) => out.push_str(&b.value),
            None => {
                if valid && !unbound.iter().any(|u| u == name) {
                    unbound.push(name.to_string());
                }
                out.push_str(&rest[open..open + 2 + close + 2]);
            }
        }
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    (out, unbound)
}

/// A short content hash: the first 12 hex digits of SHA-256.
pub fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// The hash an approval binds: the shell and the display command, so neither an edit nor a
/// different interpreter can ride on an old approval.
pub fn command_hash(shell: &str, display: &str) -> String {
    let mut bytes = Vec::with_capacity(shell.len() + display.len() + 1);
    bytes.extend_from_slice(shell.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(display.as_bytes());
    short_hash(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(pairs: &[(&str, &str, bool)]) -> Bindings {
        Bindings {
            values: pairs
                .iter()
                .map(|(k, v, s)| {
                    (
                        k.to_string(),
                        Binding {
                            value: v.to_string(),
                            secret: *s,
                        },
                    )
                })
                .collect(),
            missing: vec![],
        }
    }

    #[test]
    fn secrets_are_masked_in_display_and_present_in_exec() {
        let bindings = b(&[("host", "db.prod", false), ("pw", "hunter2", true)]);
        let text = "psql -h {{host}} -p {{ pw }} {{nope}}";
        let (shown, unbound) = subst(text, &bindings, Form::Display);
        assert_eq!(shown, "psql -h db.prod -p ••• {{nope}}");
        assert_eq!(unbound, ["nope"]);
        let (exec, _) = subst(text, &bindings, Form::Exec);
        assert_eq!(exec, "psql -h db.prod -p hunter2 {{nope}}");
        assert_eq!(bindings.secrets(), ["hunter2"]);
    }

    #[test]
    fn the_hash_form_names_the_secret_never_its_value() {
        let bindings = b(&[("pw", "hunter2", true), ("admin", "root-pw", true)]);
        let one = subst("x {{pw}}", &bindings, Form::Hash).0;
        let two = subst("x {{admin}}", &bindings, Form::Hash).0;
        assert_eq!(one, "x {{secret:pw}}");
        assert_ne!(one, two);
        assert!(!one.contains("hunter2"));
    }

    #[test]
    fn the_hash_moves_with_the_command_and_the_shell_but_not_the_secret() {
        let a = command_hash("sh", "echo 1");
        assert_ne!(a, command_hash("sh", "echo 2"));
        assert_ne!(a, command_hash("bash", "echo 1"));
        let one = b(&[("pw", "a", true)]);
        let two = b(&[("pw", "b", true)]);
        assert_eq!(
            subst("x {{pw}}", &one, Form::Display).0,
            subst("x {{pw}}", &two, Form::Display).0
        );
        assert_eq!(a.len(), 12);
    }

    #[test]
    fn defaults_are_literal_and_required_without_default_is_missing() {
        let book = Runbook::parse(
            "---\nparams:\n  env: { required: true }\n  host: { default: \"db.{{env}}\" }\n---\n",
        );
        let bindings = Bindings::defaults(&book);
        assert_eq!(bindings.values["host"].value, "db.{{env}}");
        assert_eq!(bindings.missing, ["env: required and not bound"]);
    }
}
