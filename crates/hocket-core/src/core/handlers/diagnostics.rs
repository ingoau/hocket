//! Developer diagnostics across the Connect room (see
//! [`crate::connect::diagnostics`]): collecting the other devices' bundles
//! for `Query::RoomDiagnostics`, and answering their requests for ours.

use tokio::sync::oneshot;

use crate::api::*;
use crate::connect::diagnostics::{self, Collection, ReplyPart};
use crate::connect::Input;
use crate::core::actor::Actor;
use crate::settings::keys;

/// A request from another device, waiting for the platform's bundle.
#[derive(Debug, Clone)]
pub(crate) struct PendingAnswer {
    /// The request to answer (the newest from that device).
    pub request_id: String,
    /// The request the platform was asked to build a bundle for.
    pub platform_request_id: String,
    pub to: DeviceId,
    pub deadline: f64,
}

impl Actor {
    fn developer_mode(&self) -> bool {
        self.settings.get_bool(keys::DEVELOPER_MODE)
    }

    /// `Query::RoomDiagnostics`: ask the room, answer once everyone has (or on timeout). Progress
    /// goes out as [`Event::RoomDiagnosticsChanged`] along the way.
    pub(crate) fn room_diagnostics_query(&mut self, reply: oneshot::Sender<QueryResult>) {
        let request_id = crate::util::new_id();
        let unavailable = if !self.developer_mode() {
            Some((
                RoomDiagnosticsUnavailable::DeveloperModeOff,
                "Other devices: not collected (developer mode is off on this device).\n",
            ))
        } else if self.engine.is_none() {
            Some((
                RoomDiagnosticsUnavailable::NotConnected,
                "Other devices: not collected (Connect is not running).\n",
            ))
        } else {
            None
        };
        if let Some((why, text)) = unavailable {
            self.emit(Event::RoomDiagnosticsChanged {
                status: RoomDiagnosticsStatus {
                    request_id,
                    done: true,
                    elapsed_ms: 0.0,
                    unavailable: Some(why),
                    room_answered: false,
                    room_declined: None,
                    coordinator: None,
                    coordinator_log: false,
                    devices: vec![],
                },
            });
            let _ = reply.send(QueryResult::Text(text.into()));
            return;
        }
        let now = self.now();
        tracing::info!(target: "hocket::connect", request_id, "collecting diagnostics from the room");
        let collection = Collection::new(request_id.clone(), now);
        self.emit(Event::RoomDiagnosticsChanged {
            status: collection.status(now, false),
        });
        self.diagnostics_collections.push((collection, reply));
        self.engine_input(Input::RequestDiagnostics { request_id });
    }

    pub(crate) fn on_diagnostics_reply(&mut self, part: ReplyPart) {
        let Some(i) = self
            .diagnostics_collections
            .iter()
            .position(|(c, _)| c.request_id == part.request_id)
        else {
            return;
        };
        self.diagnostics_collections[i].0.add(part);
        if self.diagnostics_collections[i].0.is_complete() {
            let (c, reply) = self.diagnostics_collections.remove(i);
            self.finish_collection(c, reply);
        } else {
            let status = self.diagnostics_collections[i].0.status(self.now(), false);
            self.emit(Event::RoomDiagnosticsChanged { status });
        }
    }

    /// The final status, then the answer. A request the room turned away (asked again within its
    /// 10 s) gets the collection before it, status and text, when there is one: nobody else
    /// answered, so it would only have lost what that one gathered.
    fn finish_collection(&mut self, c: Collection, reply: oneshot::Sender<QueryResult>) {
        let now = self.now();
        let status = c.status(now, true);
        let (status, text) = match (&status.room_declined, &self.last_room_diagnostics) {
            (Some(_), Some((last, text))) => (last.clone(), text.clone()),
            _ => {
                let text = c.render(now);
                if status.room_answered && status.room_declined.is_none() {
                    self.last_room_diagnostics = Some((status.clone(), text.clone()));
                }
                (status, text)
            }
        };
        self.emit(Event::RoomDiagnosticsChanged { status });
        let _ = reply.send(QueryResult::Text(text));
    }

    /// Another device asks for our bundle: decline unless developer mode is on here, else ask
    /// the platform for it (with the core's own as the fallback).
    pub(crate) fn on_diagnostics_requested(&mut self, request_id: String, from: DeviceId) {
        let from_device = self
            .engine
            .as_ref()
            .and_then(|e| e.devices().into_iter().find(|d| d.id == from))
            .map(|d| d.name)
            .unwrap_or_else(|| from.clone());
        if !self.developer_mode() {
            tracing::info!(target: "hocket::connect", from = %from_device, "diagnostics request declined: developer mode is off");
            self.queue_input(Input::AnswerDiagnostics {
                request_id,
                to: from,
                text: String::new(),
                declined: Some("developer mode is off on this device".into()),
            });
            return;
        }
        if let Some(a) = self.diagnostics_answers.iter_mut().find(|a| a.to == from) {
            // Still building the previous answer for it: that one answers this request instead
            // (the asking device only waits for the newest).
            a.request_id = request_id;
            return;
        }
        tracing::info!(target: "hocket::connect", from = %from_device, "diagnostics requested by another device");
        self.diagnostics_answers.push(PendingAnswer {
            request_id: request_id.clone(),
            platform_request_id: request_id.clone(),
            to: from,
            deadline: self.now() + diagnostics::PLATFORM_ANSWER_TIMEOUT_MS,
        });
        self.emit(Event::DiagnosticsRequested {
            request_id,
            from_device,
        });
    }

    /// `Command::AnswerDiagnosticsRequest`: the platform's bundle.
    pub(crate) fn answer_diagnostics_request(&mut self, request_id: String, text: String) {
        let Some(i) = self
            .diagnostics_answers
            .iter()
            .position(|a| a.platform_request_id == request_id)
        else {
            return; // too late: the core's own answer went already
        };
        let a = self.diagnostics_answers.remove(i);
        let (text, declined) = self.unless_turned_off(text);
        self.engine_input(Input::AnswerDiagnostics {
            request_id: a.request_id,
            to: a.to,
            text,
            declined,
        });
    }

    /// Developer mode may have gone off while the bundle was being built: then nothing is sent.
    fn unless_turned_off(&self, text: String) -> (String, Option<String>) {
        if self.developer_mode() {
            (text, None)
        } else {
            (
                String::new(),
                Some("developer mode is off on this device".into()),
            )
        }
    }

    /// Timeouts: collections answer with what arrived; requests the platform left unanswered
    /// get the core's own diagnostics and log.
    pub(crate) fn diagnostics_tick(&mut self, now: f64) {
        while let Some(i) = self
            .diagnostics_collections
            .iter()
            .position(|(c, _)| c.expired(now))
        {
            let (c, reply) = self.diagnostics_collections.remove(i);
            self.finish_collection(c, reply);
        }
        while let Some(i) = self
            .diagnostics_answers
            .iter()
            .position(|a| now >= a.deadline)
        {
            let a = self.diagnostics_answers.remove(i);
            if !self.developer_mode() {
                let (text, declined) = self.unless_turned_off(String::new());
                self.engine_input(Input::AnswerDiagnostics {
                    request_id: a.request_id,
                    to: a.to,
                    text,
                    declined,
                });
                continue;
            }
            let text = format!(
                "(the app did not build its bundle in time: the core's own diagnostics and log follow)\n\n{}\nCore log\n--------\n{}",
                self.diagnostics(),
                crate::devlog::recent(crate::devlog::WINDOW_MS)
            );
            self.engine_input(Input::AnswerDiagnostics {
                request_id: a.request_id,
                to: a.to,
                text,
                declined: None,
            });
        }
    }
}
