//! The runbook family on the window's side: sending a request about a `*.runbook.md`, applying what
//! the host says about one, and the run gate's human half — every Run here is a person's click
//! (or key) on the command they were shown.

use gpui::Context;
use kubiq_proto::messages::Message;
use kubiq_proto::runbook::{Approval, OutputQuery, RunOptions, RunbookEvent, RunbookRequest};

use super::AppState;
use crate::state::editor::ViewerKind;
use crate::state::runbook::{Confirm, Key, runbooks};

impl AppState {
    /// Whether the drone serving this runbook's project is too old to run runbooks (it does not
    /// advertise `CAP_RUNBOOK`): the viewer says so instead of waiting for an answer.
    pub fn runbook_unsupported(&self, key: &Key) -> bool {
        self.bus
            .serving_drone_lacks(key.0, kubiq_proto::carrier::CAP_RUNBOOK)
    }

    /// Ask the host something about a runbook.
    pub fn runbook_send(&self, key: &Key, request: RunbookRequest) {
        self.bus.send(Message::Runbook {
            project_id: key.0,
            rel: key.1.clone(),
            request,
        });
    }

    /// Run `cells` as the human approved them, at the hashes the snapshot showed. A step that
    /// waits on its prerequisites, or a cell with `confirm=`, asks first: the confirmation bar
    /// sends the run with the flags it names. `request` is the agent request this answers.
    pub fn runbook_run(
        &mut self,
        key: &Key,
        cells: &[String],
        request: Option<u64>,
        confirm_text: &dyn Fn(&str) -> Option<String>,
        cx: &mut Context<Self>,
    ) {
        let defaults = self.cells_settings().runbook.clone();
        let Some(tab) = runbooks(cx).tabs.get_mut(key) else {
            return;
        };
        let Some(snap) = tab.snapshot.as_ref() else {
            return;
        };
        let approvals: Vec<Approval> = cells
            .iter()
            .filter_map(|c| {
                snap.commands.get(c).map(|cmd| Approval {
                    cell: c.clone(),
                    hash: cmd.hash.clone(),
                })
            })
            .collect();
        let mut reasons = Vec::new();
        let mut skip_prereqs = false;
        for c in cells {
            if let Some(step) = snap.states.step(c)
                && !step.waits_on.is_empty()
            {
                skip_prereqs = true;
                reasons.push(format!(
                    "#{c} waits on {} — run it without its prerequisites?",
                    step.waits_on
                        .iter()
                        .map(|w| format!("#{w}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(text) = confirm_text(c) {
                reasons.push(format!("#{c}: {text}"));
            }
        }
        let options = RunOptions {
            request,
            default_timeout_secs: defaults.timeout_secs,
            log_cap_mib: defaults.log_cap_mib,
            ..RunOptions::default()
        };
        if reasons.is_empty() {
            self.runbook_send(key, RunbookRequest::Run { approvals, options });
        } else {
            tab.confirm = Some(Confirm {
                approvals,
                options: RunOptions {
                    skip_prereqs,
                    confirmed: true,
                    ..options
                },
                message: reasons.join("\n"),
            });
        }
        cx.notify();
    }

    /// Open an attempt's retained log in the zoom dialog: the host answers `Output`.
    pub fn runbook_full_output(&self, key: &Key, cell: &str, attempt: u64) {
        self.runbook_send(
            key,
            RunbookRequest::ReadOutput {
                cell: cell.to_string(),
                attempt,
                query: OutputQuery::All,
            },
        );
    }

    /// The window was told something about a runbook. Answers with the message when it is not
    /// this family's.
    pub(super) fn receive_runbook(
        &mut self,
        message: Message,
        cx: &mut Context<Self>,
    ) -> Option<Message> {
        let Message::RunbookEvent {
            project_id,
            rel,
            event,
        } = message
        else {
            return Some(message);
        };
        let key = (project_id, rel);
        let mut open = None;
        let mut zoom = None;
        {
            let Some(tab) = runbooks(cx).tabs.get_mut(&key) else {
                return None;
            };
            match *event {
                RunbookEvent::State { snapshot, bound } => {
                    // A tail whose attempt is no longer running is done; the excerpt replaces it.
                    let running: Vec<u64> = snapshot
                        .attempts
                        .iter()
                        .filter(|a| a.status == "running")
                        .map(|a| a.attempt)
                        .collect();
                    tab.tails.retain(|_, ring| running.contains(&ring.attempt));
                    tab.snapshot = Some(snapshot);
                    tab.bound = bound;
                }
                RunbookEvent::Tail(tail) => {
                    tab.tails.entry(tail.cell.clone()).or_default().push(tail);
                }
                RunbookEvent::Finished { cell, label, .. } => {
                    tab.notice = Some((format!("#{cell} finished: {label}"), false));
                }
                RunbookEvent::Ran(report) => {
                    if !report.refused.is_empty() {
                        let text = report
                            .refused
                            .iter()
                            .map(|(_, why)| why.clone())
                            .collect::<Vec<_>>()
                            .join("\n");
                        tab.notice = Some((text, true));
                    }
                }
                RunbookEvent::Output {
                    cell,
                    attempt,
                    slice,
                    error,
                } => match (slice, error) {
                    (Some(slice), _) => {
                        let mut facts = format!(
                            "#{cell} · attempt {attempt} · {} lines · {} bytes",
                            slice.total_lines, slice.total_bytes
                        );
                        if slice.truncated {
                            facts.push_str(" · the log was capped");
                        }
                        if slice.capped {
                            facts.push_str(" · showing the first 2 MB");
                        }
                        zoom = Some((format!("#{cell} — output"), facts, slice.text));
                    }
                    (None, Some(error)) => tab.notice = Some((error, true)),
                    _ => {}
                },
                RunbookEvent::ActionDone {
                    action,
                    outcome,
                    error,
                } => match (outcome, error) {
                    (Some(outcome), _) => {
                        tab.notice = Some((outcome.message.clone(), false));
                        if let Some(report) = outcome.report {
                            tab.report = Some((action, report));
                        }
                        open = outcome.open;
                        tab.form = None;
                        tab.form_inputs.clear();
                    }
                    (None, Some(error)) => tab.notice = Some((error, true)),
                    _ => {}
                },
                RunbookEvent::Refused { reason } => tab.notice = Some((reason, true)),
            }
        }
        if let Some((title, facts, text)) = zoom {
            crate::ui::cells::zoom::open_text(cx, title, facts, text, None);
        }
        if let Some(rel) = open {
            self.open_file_with(
                rel,
                ViewerKind::Contributed(crate::ext::ids::RUNBOOK_VIEWER),
                cx,
            );
        }
        cx.notify();
        None
    }
}
