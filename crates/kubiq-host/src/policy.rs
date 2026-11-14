//! Sandbox policies on the wire: the conversion between the interface's structured
//! [`PolicyDoc`] and the isol8 layer the library stores.
//!
//! Both sides spell enums as the strings isol8 writes (`"ro"`, `"subpath"`, `"mach-lookup"`), so
//! the conversion goes through a JSON value shaped like the layer's own serde form: no enum is
//! named here, and a value isol8 would reject (`access = "sometimes"`) is isol8's error, not a
//! second validation that could drift from it.

use agent_manager::policy::{Info, Layer, Origin};
use kubiq_proto::policy::{
    PolicyDoc, PolicyFilter, PolicyHome, PolicyInfo, PolicyOrigin, PolicyPath, PolicyRule,
};
use serde_json::{Map, Value, json};

/// The wire row for a store row.
pub fn info(info: Info) -> PolicyInfo {
    PolicyInfo {
        name: info.name,
        origin: match info.origin {
            Origin::Builtin => PolicyOrigin::Builtin,
            Origin::Local => PolicyOrigin::Local,
            Origin::Imported { source } => PolicyOrigin::Imported { source },
            Origin::User => PolicyOrigin::User,
        },
        description: info.description,
        default: info.default,
    }
}

fn path_value(path: &PolicyPath) -> Value {
    let mut grant = json!({ "path": path.path, "access": path.access });
    // Unset is isol8's default, `subpath`; writing nothing keeps the file as a person would.
    if !path.matching.is_empty() {
        grant["match"] = json!(path.matching);
    }
    grant
}

fn filter_value(filter: &PolicyFilter) -> Value {
    json!({ "os": filter.os, "arch": filter.arch, "executables": filter.executables })
}

/// The isol8 layer a [`PolicyDoc`] describes, or why isol8 would refuse it.
pub fn layer_of(doc: &PolicyDoc) -> Result<Layer, String> {
    let mut layer = Map::new();
    layer.insert("requires".into(), json!(doc.requires));
    if let Some(filter) = &doc.filter {
        layer.insert("filter".into(), filter_value(filter));
    }
    layer.insert(
        "paths".into(),
        Value::Array(doc.paths.iter().map(path_value).collect()),
    );
    let env: Map<String, Value> = doc
        .env
        .iter()
        .map(|(name, value)| (name.clone(), json!(value)))
        .collect();
    layer.insert("env".into(), Value::Object(env));
    if let Some(home) = &doc.home_replace {
        let mut table = json!({
            "enabled": home.enabled,
            "auto_scratch": home.auto_scratch,
            "seed": home.seed,
        });
        if let Some(path) = &home.path {
            table["path"] = json!(path);
        }
        layer.insert("home_replace".into(), table);
    }
    if !doc.ensure_args.is_empty() {
        layer.insert("rewrite".into(), json!({ "ensure_args": doc.ensure_args }));
    }
    layer.insert(
        "policies".into(),
        Value::Array(
            doc.rules
                .iter()
                .map(|rule| {
                    json!({
                        "filter": filter_value(&rule.filter),
                        "paths": rule.paths.iter().map(path_value).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        ),
    );
    if !doc.macos_capabilities.is_empty() || !doc.macos_raw.is_empty() {
        layer.insert(
            "macos".into(),
            json!({ "capabilities": doc.macos_capabilities, "raw": doc.macos_raw }),
        );
    }
    if !doc.windows_capabilities.is_empty() {
        layer.insert(
            "windows".into(),
            json!({ "capabilities": doc.windows_capabilities }),
        );
    }
    serde_json::from_value(Value::Object(layer)).map_err(|error| error.to_string())
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn paths(value: &Value) -> Vec<PolicyPath> {
    value
        .get("paths")
        .and_then(Value::as_array)
        .map(|grants| {
            grants
                .iter()
                .map(|grant| PolicyPath {
                    path: text(grant, "path").unwrap_or_default(),
                    access: text(grant, "access").unwrap_or_default(),
                    matching: text(grant, "match").unwrap_or_else(|| "subpath".into()),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn filter_of(value: &Value) -> PolicyFilter {
    PolicyFilter {
        os: strings(value, "os"),
        arch: strings(value, "arch"),
        executables: strings(value, "executables"),
    }
}

/// The structured form of an isol8 layer, for the editor.
pub fn doc_of(layer: &Layer) -> PolicyDoc {
    // A `Profile` always serializes; the null is unreachable and reads as an empty layer.
    let value = serde_json::to_value(layer).unwrap_or(Value::Null);
    let mut env: Vec<(String, String)> = value
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            env.iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    env.sort();
    let section = |name: &str| value.get(name).filter(|it| it.is_object());
    PolicyDoc {
        requires: strings(&value, "requires"),
        filter: section("filter").map(filter_of),
        paths: paths(&value),
        env,
        home_replace: section("home_replace").map(|home| PolicyHome {
            enabled: home
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            auto_scratch: home
                .get("auto_scratch")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            path: text(home, "path"),
            seed: strings(home, "seed"),
        }),
        ensure_args: section("rewrite")
            .map(|rewrite| strings(rewrite, "ensure_args"))
            .unwrap_or_default(),
        rules: value
            .get("policies")
            .and_then(Value::as_array)
            .map(|rules| {
                rules
                    .iter()
                    .map(|rule| PolicyRule {
                        filter: rule.get("filter").map(filter_of).unwrap_or_default(),
                        paths: paths(rule),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        macos_capabilities: section("macos")
            .map(|macos| strings(macos, "capabilities"))
            .unwrap_or_default(),
        macos_raw: section("macos")
            .and_then(|macos| text(macos, "raw"))
            .unwrap_or_default(),
        windows_capabilities: section("windows")
            .map(|windows| strings(windows, "capabilities"))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(path: &str, access: &str, matching: &str) -> PolicyPath {
        PolicyPath {
            path: path.into(),
            access: access.into(),
            matching: matching.into(),
        }
    }

    fn full() -> PolicyDoc {
        PolicyDoc {
            requires: vec!["base".into()],
            filter: Some(PolicyFilter {
                os: vec!["macos".into()],
                arch: vec!["aarch64".into()],
                executables: vec!["claude".into()],
            }),
            paths: vec![
                grant("~/.cache", "rw", "subpath"),
                grant("~/.ssh", "none", "literal"),
            ],
            env: vec![("A".into(), "1".into()), ("B".into(), "2".into())],
            home_replace: Some(PolicyHome {
                enabled: true,
                auto_scratch: true,
                path: Some("~/scratch".into()),
                seed: vec![".gitconfig".into()],
            }),
            ensure_args: vec!["--flag".into()],
            rules: vec![PolicyRule {
                filter: PolicyFilter {
                    os: vec!["windows".into()],
                    ..Default::default()
                },
                paths: vec![grant("%APPDATA%\\x", "ro", "literal")],
            }],
            macos_capabilities: vec!["mach-lookup".into(), "pseudo-tty".into()],
            macos_raw: "(allow default)".into(),
            windows_capabilities: vec!["internet-client".into()],
        }
    }

    #[test]
    fn a_doc_round_trips_through_a_layer() {
        let doc = full();
        let layer = layer_of(&doc).expect("a valid doc");
        assert_eq!(doc_of(&layer), doc);
    }

    #[test]
    fn an_empty_doc_is_an_empty_layer() {
        let doc = PolicyDoc::default();
        assert_eq!(doc_of(&layer_of(&doc).unwrap()), doc);
    }

    #[test]
    fn a_layer_survives_its_own_toml() {
        let layer = layer_of(&full()).unwrap();
        let toml = toml::to_string_pretty(&layer).unwrap();
        let back: Layer = toml::from_str(&toml).unwrap();
        assert_eq!(doc_of(&back), full());
    }

    #[test]
    fn isol8_refuses_what_it_would_refuse() {
        let mut doc = full();
        doc.paths[0].access = "sometimes".into();
        assert!(layer_of(&doc).is_err());
    }
}
