//! A destination written down and read back.
//!
//! The round trip is the whole contract: whatever `Display` prints, `FromStr` has to return the
//! same value for, or a bookmark from three weeks ago lands somewhere else. Everything here is
//! arithmetic over ids and bytes — no window, no host.

use std::str::FromStr;

use kubiq::state::dock::ChatId;
use kubiq::state::editor::{Subject, tab_key};
use kubiq::state::nav::{
    Destination, Locus, View, file_link, parse_link, resolve_relative, split_line_suffix,
};
use kubiq_proto::files::DiffBase;
use kubiq_proto::ids::{PaneId, ProjectId, SessionId, TaskId};
use kubiq_proto::work::AgentId;

use kubiq::state::teams::{TeamsInspectorTab, TeamsSelection};

fn project() -> ProjectId {
    ProjectId::generate()
}

/// Every view arm, one of each shape the parser has to tell apart.
fn views() -> Vec<View> {
    vec![
        View::Control,
        View::Kb,
        View::Git,
        View::Db,
        View::Logs,
        View::Ide {
            key: "crates/kubiq/src/app/nav.rs".into(),
        },
        View::Ide {
            key: tab_key("README.md", Subject::Diff(DiffBase::Head)),
        },
        View::Explorer {
            path: "_docs/features".into(),
        },
        View::Terminal {
            pane: PaneId::generate(),
        },
        View::Teams {
            selection: TeamsSelection::Session(SessionId::generate()),
            tab: TeamsInspectorTab::Chat,
        },
        View::Teams {
            selection: TeamsSelection::Agent(AgentId::generate()),
            tab: TeamsInspectorTab::Tasks,
        },
        View::Teams {
            selection: TeamsSelection::Subagent {
                agent: AgentId::generate(),
                subagent: "t751".into(),
            },
            tab: TeamsInspectorTab::Chat,
        },
        View::Agents {
            agent: AgentId::generate(),
        },
        View::Tasks {
            task: TaskId::generate(),
        },
        View::Chat {
            chat: ChatId::generate(),
        },
        View::Help {
            page: "git-changes".into(),
        },
    ]
}

fn loci() -> Vec<Option<Locus>> {
    vec![
        None,
        Some(Locus::Line { line: 1712 }),
        Some(Locus::Span { from: 10, to: 24 }),
        Some(Locus::Anchor {
            slug: "catalogue".into(),
        }),
        Some(Locus::Viewport {
            x: -120.5,
            y: 40.0,
            scale: 1.25,
        }),
        Some(Locus::Node { key: "n-7".into() }),
    ]
}

#[test]
fn every_arm_and_every_locus_round_trips() {
    let project = project();
    for view in views() {
        for locus in loci() {
            let dest = Destination {
                project,
                view: view.clone(),
                locus,
            };
            let text = dest.to_string();
            let back = Destination::from_str(&text)
                .unwrap_or_else(|_| panic!("{text} did not parse back"));
            assert_eq!(back, dest, "{text}");
        }
    }
}

#[test]
fn the_written_examples_parse_as_what_they_say() {
    let id = project();
    let task = TaskId::generate();
    let agent = AgentId::generate();
    let cases: Vec<(String, Destination)> = vec![
        (
            format!("kubiq://{id}/ide/crates/kubiq/src/app/wire.rs#L1712"),
            Destination {
                project: id,
                view: View::Ide {
                    key: "crates/kubiq/src/app/wire.rs".into(),
                },
                locus: Some(Locus::Line { line: 1712 }),
            },
        ),
        (
            format!("kubiq://{id}/ide/README.md#L10-24"),
            Destination {
                project: id,
                view: View::Ide {
                    key: "README.md".into(),
                },
                locus: Some(Locus::Span { from: 10, to: 24 }),
            },
        ),
        (
            format!("kubiq://{id}/ide/_docs/INDEX.md#catalogue"),
            Destination {
                project: id,
                view: View::Ide {
                    key: "_docs/INDEX.md".into(),
                },
                locus: Some(Locus::Anchor {
                    slug: "catalogue".into(),
                }),
            },
        ),
        (
            format!("kubiq://{id}/tasks/{task}"),
            Destination::new(id, View::Tasks { task }),
        ),
        // The proposal wrote this one as `agents/<id>/chat`, from before the arm split: the
        // agents screen *is* the transcript, and the inspector tab belongs to the graph.
        (
            format!("kubiq://{id}/agents/{agent}"),
            Destination::new(id, View::Agents { agent }),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(Destination::from_str(&text).ok(), Some(want), "{text}");
    }
}

#[test]
fn a_reversed_span_is_the_same_two_lines() {
    let id = project();
    let dest = Destination::from_str(&format!("kubiq://{id}/ide/a.rs#L24-10")).unwrap();
    assert_eq!(dest.locus, Some(Locus::Span { from: 10, to: 24 }));
}

#[test]
fn junk_is_not_a_link() {
    let id = project();
    for text in [
        "https://example.com/a".to_string(),
        "kubiq://not-a-ulid/logs".to_string(),
        format!("kubiq://{id}/nowhere"),
        // A slug that names nothing takes nothing: a trailing segment is a different string.
        format!("kubiq://{id}/logs/extra"),
        format!("kubiq://{id}/ide/a%zz.rs"),
        format!("kubiq://{id}/ide/a.rs#L0"),
        // One segment means one.
        format!("kubiq://{id}/tasks/{}/extra", TaskId::generate()),
        // The selection prefix is forced.
        format!("kubiq://{id}/teams/{}", SessionId::generate()),
        // `[Teams]` is gone (T-330), and its `graph` links with it.
        format!("kubiq://{id}/graph/s:{}", SessionId::generate()),
        // A session has no delegate: `s:` carrying a third segment is a different string, not a
        // refinement of the session selection.
        format!("kubiq://{id}/teams/s:{}/chat/t751", SessionId::generate()),
    ] {
        assert!(Destination::from_str(&text).is_err(), "{text} parsed");
    }
}

/// The Teams grammar, `kubiq://<project>/teams/<s|a>:<ulid>[/<chat|tasks>[/<subagent>]]` —
/// a selection and a tab, plus the delegate segment only an agent selection may carry.
#[test]
fn the_teams_grammar_round_trips_and_refuses_a_delegate_on_a_session() {
    let id = project();
    let session = SessionId::generate();
    let agent = AgentId::generate();

    // A session selection prints and reads back with no third segment at all.
    let dest = Destination::new(
        id,
        View::Teams {
            selection: TeamsSelection::Session(session),
            tab: TeamsInspectorTab::Chat,
        },
    );
    let text = dest.to_string();
    assert_eq!(text, format!("kubiq://{id}/teams/s:{session}/chat"));
    assert_eq!(Destination::from_str(&text).unwrap(), dest);

    // An agent selection with a delegate prints the delegate as a third segment, unsplit — a
    // `Task` call's id is the harness's own string, and the grammar makes no promise about what
    // is in it besides a `/`.
    let dest = Destination::new(
        id,
        View::Teams {
            selection: TeamsSelection::Subagent {
                agent,
                subagent: "t751".into(),
            },
            tab: TeamsInspectorTab::Tasks,
        },
    );
    let text = dest.to_string();
    assert_eq!(text, format!("kubiq://{id}/teams/a:{agent}/tasks/t751"));
    assert_eq!(Destination::from_str(&text).unwrap(), dest);

    // A session cannot carry a delegate,
    // because a delegate always names the agent that spawned it.
    let text = format!("kubiq://{id}/teams/s:{session}/chat/t751");
    assert!(
        Destination::from_str(&text).is_err(),
        "{text} parsed as a link"
    );
}

#[test]
fn a_path_carrying_the_grammars_own_characters_survives() {
    let id = project();
    let key = "_docs/a #1 100% done.md".to_string();
    let dest = Destination {
        project: id,
        view: View::Ide { key: key.clone() },
        locus: Some(Locus::Line { line: 3 }),
    };
    let text = dest.to_string();
    assert!(
        text.contains("%23") && text.contains("%25") && text.contains("%20"),
        "{text}"
    );
    assert_eq!(Destination::from_str(&text).unwrap(), dest);
}

#[test]
fn utf8_is_left_readable() {
    let id = project();
    let dest = Destination::new(
        id,
        View::Ide {
            key: "_docs/café.md".into(),
        },
    );
    assert!(dest.to_string().contains("café"));
    assert_eq!(Destination::from_str(&dest.to_string()).unwrap(), dest);
}

#[test]
fn an_anchor_that_looks_like_another_locus_says_it_is_an_anchor() {
    let id = project();
    for slug in ["L42", "L10-24", "v=1,2,3", "n=key", "a=x"] {
        let dest = Destination {
            project: id,
            view: View::Ide { key: "a.md".into() },
            locus: Some(Locus::Anchor { slug: slug.into() }),
        };
        let text = dest.to_string();
        assert!(text.contains("#a="), "{text}");
        assert_eq!(Destination::from_str(&text).unwrap(), dest, "{text}");
    }
}

#[test]
fn a_path_that_leaves_the_project_is_not_a_link() {
    let id = project();
    for path in ["../secrets", "/etc/passwd", "a/../../b"] {
        let text = format!("kubiq://{id}/ide/{path}");
        assert!(Destination::from_str(&text).is_err(), "{text} parsed");
    }
}

// ── relative links, as a document writes them ──────────────────────────────

fn relative(target: &str) -> Option<Destination> {
    resolve_relative(project(), "_docs/x.md", target)
}

fn key_of(dest: &Destination) -> String {
    match &dest.view {
        View::Ide { key } => key.clone(),
        other => panic!("not a file: {other:?}"),
    }
}

#[test]
fn a_relative_link_walks_from_the_documents_folder() {
    let up = relative("../src/app.rs#L200").unwrap();
    assert_eq!(key_of(&up), "src/app.rs");
    assert_eq!(up.locus, Some(Locus::Line { line: 200 }));

    // `.` and a bare name are the same thing: the document's own folder.
    assert_eq!(
        key_of(&relative("./sibling.md").unwrap()),
        "_docs/sibling.md"
    );
    assert_eq!(key_of(&relative("sibling.md").unwrap()), "_docs/sibling.md");
}

#[test]
fn escaping_the_project_root_is_not_a_link() {
    assert_eq!(relative("../../../.."), None);
    assert_eq!(relative("/etc/passwd"), None);
}

#[test]
fn a_bare_fragment_is_the_same_document() {
    let id = project();
    let here = resolve_relative(id, "_docs/x.md", "#heading").unwrap();
    assert_eq!(key_of(&here), "_docs/x.md");
    assert_eq!(
        here.locus,
        Some(Locus::Anchor {
            slug: "heading".into()
        })
    );
}

#[test]
fn somewhere_else_entirely_is_left_to_the_operating_system() {
    assert_eq!(relative("https://example.com"), None);
    assert_eq!(relative("http://example.com"), None);
    assert_eq!(relative("mailto:someone@example.com"), None);
}

#[test]
fn a_full_link_inside_a_document_wins_over_the_document() {
    let elsewhere = ProjectId::generate();
    let written = format!("kubiq://{elsewhere}/tasks/{}", TaskId::generate());
    let dest = resolve_relative(project(), "_docs/x.md", &written).unwrap();
    assert_eq!(dest.project, elsewhere);
    assert_eq!(dest.to_string(), written);
}

/// The project-less form: *this view, in the current project*. What a help page writes, because
/// content compiled into the binary has never seen a project's ULID.
#[test]
fn the_project_less_form_takes_the_project_it_is_read_in() {
    let here = project();
    let dest = parse_link("kubiq://./git", Some(here)).expect("a link");
    assert_eq!(dest.project, here);
    assert_eq!(dest.view, View::Git);

    let page = parse_link("kubiq://./help/git-changes#staging", Some(here)).expect("a link");
    assert_eq!(
        page.view,
        View::Help {
            page: "git-changes".into()
        }
    );
    assert_eq!(
        page.locus,
        Some(Locus::Anchor {
            slug: "staging".into()
        })
    );
}

/// And with no project open it is inert — the same nothing every other unrecognised target is.
#[test]
fn the_project_less_form_is_inert_with_no_project() {
    assert!(parse_link("kubiq://./git", None).is_err());
    assert!(Destination::from_str("kubiq://./git").is_err());
}

/// A document's own link to a help page resolves against the project the document is read in.
#[test]
fn a_document_may_link_to_a_help_page_without_naming_a_project() {
    let here = project();
    let dest = resolve_relative(here, "_docs/x.md", "kubiq://./help/index").unwrap();
    assert_eq!(dest.project, here);
    assert_eq!(
        dest.view,
        View::Help {
            page: "index".into()
        }
    );
}

// ── paths with a place in them, as a compiler or an agent writes them ──────

#[test]
fn a_line_suffix_names_a_line_and_drops_the_column() {
    let line = |line| Some(Locus::Line { line });
    assert_eq!(split_line_suffix("src/app.rs:42"), ("src/app.rs", line(42)));
    assert_eq!(
        split_line_suffix("src/app.rs:42:7"),
        ("src/app.rs", line(42))
    );
    assert_eq!(split_line_suffix("src/app.rs"), ("src/app.rs", None));
    // Line zero is not a line, and a suffix alone is not a path.
    assert_eq!(split_line_suffix("src/app.rs:0"), ("src/app.rs:0", None));
    assert_eq!(split_line_suffix(":12"), (":12", None));
    assert_eq!(split_line_suffix("a:b"), ("a:b", None));
}

#[test]
fn a_relative_link_may_carry_a_line_suffix() {
    let deep = relative("../src/app.rs:12:3").unwrap();
    assert_eq!(key_of(&deep), "src/app.rs");
    assert_eq!(deep.locus, Some(Locus::Line { line: 12 }));
    // A bare `name:12` reads as a URI scheme; it is a path after all.
    let bare = relative("sibling.md:5").unwrap();
    assert_eq!(key_of(&bare), "_docs/sibling.md");
    assert_eq!(bare.locus, Some(Locus::Line { line: 5 }));
    // A real scheme with a port stays somewhere else.
    assert_eq!(relative("http://localhost:8080"), None);
    // A bare `word:digits` is a host and port, not a file at a line.
    assert_eq!(relative("localhost:8080"), None);
}

#[test]
fn an_absolute_path_is_a_file_link() {
    let line = |line| Some(Locus::Line { line });
    assert_eq!(
        file_link("/abs/src/app.rs"),
        Some(("/abs/src/app.rs".to_string(), None))
    );
    assert_eq!(
        file_link("/abs/src/app.rs:42:7"),
        Some(("/abs/src/app.rs".to_string(), line(42)))
    );
    assert_eq!(
        file_link("/abs/src/app.rs#L9"),
        Some(("/abs/src/app.rs".to_string(), line(9)))
    );
    assert_eq!(
        file_link("file:///abs/my%20file.md:3"),
        Some(("/abs/my file.md".to_string(), line(3)))
    );
    assert_eq!(
        file_link("file://localhost/abs/a.rs"),
        Some(("/abs/a.rs".to_string(), None))
    );
    // Another host's file, a relative path, and the web are not.
    assert_eq!(file_link("file://server/abs/a.rs"), None);
    assert_eq!(file_link("src/app.rs:4"), None);
    assert_eq!(file_link("https://example.com/a"), None);
}
