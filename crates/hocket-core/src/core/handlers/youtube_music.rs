//! YouTube Music accounts: anonymous or signed in on this device
//! (`docs/providers.md`, Phase 4), and the bridge to the platform's JS host.

use std::sync::Arc;

use futures::future::BoxFuture;

use crate::api::*;
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};
use crate::db::sync::LibrarySync;
use crate::provider::Provider;
use crate::ytm::auth::Cookies;
use crate::ytm::client::InnerTube;
use crate::ytm::http::{ReqwestYtmHttp, YtmHttp};
use crate::ytm::player::{JsAnswer, JsHost, JsRequest};
use crate::ytm::{YtmProvider, ANON};

/// How long the platform has to answer a JS host request.
const JS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The JS host as the core sees it: requests go to the platform as events.
struct PlatformJsHost {
    tx: tokio::sync::mpsc::UnboundedSender<ActorMsg>,
}

impl JsHost for PlatformJsHost {
    fn solve(&self, req: JsRequest) -> BoxFuture<'static, Result<JsAnswer, String>> {
        let request = match req {
            JsRequest::Prepare {
                video_id,
                visitor_data,
            } => YtmJsRequest::Prepare {
                video_id,
                visitor_data,
            },
            JsRequest::Decipher { video_id, formats } => {
                YtmJsRequest::Decipher { video_id, formats }
            }
        };
        let (reply, rx) = tokio::sync::oneshot::channel();
        let sent = self
            .tx
            .send(ActorMsg::Internal(Internal::YtmJs { request, reply }))
            .is_ok();
        Box::pin(async move {
            if !sent {
                return Err("the core stopped".into());
            }
            let a = tokio::time::timeout(JS_TIMEOUT, rx)
                .await
                .map_err(|_| "the JS host didn't answer".to_string())?
                .map_err(|_| "the JS host request was dropped".to_string())??;
            Ok(JsAnswer {
                signature_timestamp: a.signature_timestamp,
                po_token: a.po_token,
                urls: a.urls,
            })
        })
    }
}

impl Actor {
    fn ytm_http(&mut self) -> Option<Arc<dyn YtmHttp>> {
        if let Some(h) = self.ytm_http.clone() {
            return Some(h);
        }
        match ReqwestYtmHttp::new() {
            Ok(h) => Some(Arc::new(h)),
            Err(e) => {
                self.log("warn", format!("YouTube Music http client: {e}"));
                None
            }
        }
    }

    pub(crate) fn add_youtube_music(&mut self, cookies: Option<Secret>, js_host: bool) {
        let Some(http) = self.ytm_http() else { return };
        let parsed = cookies.as_ref().and_then(|c| Cookies::parse(c.expose()));
        if cookies.is_some() && parsed.is_none() {
            self.toast(
                "That Google sign-in didn't include a YouTube session. Try signing in again.",
                None,
            );
            return;
        }
        let Some(parsed) = parsed else {
            let it = Arc::new(InnerTube::new(http, self.clock.clone(), None));
            self.install_youtube_music(
                ANON.into(),
                "YouTube Music".into(),
                it,
                false,
                js_host,
                None,
            );
            return;
        };
        // Check the session first: the account id is its channel id.
        let it = Arc::new(InnerTube::new(http, self.clock.clone(), Some(parsed)));
        let tx = self.tx.clone();
        self.spawn(async move {
            let result = async {
                let menu = it.account_menu().await?;
                let (channel, name) = crate::ytm::parse::parse_account(&menu).ok_or_else(|| {
                    crate::provider::ProviderError::Auth("no account in the session".into())
                })?;
                Ok::<_, crate::provider::ProviderError>((
                    channel,
                    name,
                    crate::ytm::parse::is_premium(&menu),
                ))
            }
            .await
            .map_err(|e| (e.kind(), e.to_string()));
            let _ = tx.send(ActorMsg::Internal(Internal::YtmProbed {
                cookies,
                js_host,
                result,
            }));
        });
    }

    pub(crate) fn on_ytm_probed(
        &mut self,
        cookies: Option<Secret>,
        js_host: bool,
        result: Result<(String, String, bool), (ErrorKind, String)>,
    ) {
        let parsed = cookies.as_ref().and_then(|c| Cookies::parse(c.expose()));
        match result {
            Ok((channel, name, premium)) => {
                let Some(http) = self.ytm_http() else { return };
                let it = Arc::new(InnerTube::new(http, self.clock.clone(), parsed));
                self.install_youtube_music(channel, name, it, premium, js_host, cookies);
                // Signed in now: the anonymous account goes. Its Hocket library
                // stays (it lives on the identity), and so do pins, the cache
                // and queue items: they name the one `ytm` source.
                if self.accounts.get(ANON).is_some() {
                    self.remove_account(ANON.into());
                }
            }
            Err((ErrorKind::Auth, detail)) => {
                // Expired cookies: the persisted account asks to sign in again.
                let known: Vec<AccountId> = self
                    .accounts
                    .iter()
                    .filter(|a| a.info.kind == AccountKind::YoutubeMusic && a.info.id != ANON)
                    .map(|a| a.info.id.clone())
                    .collect();
                self.log("warn", format!("YouTube Music sign-in refused: {detail}"));
                self.toast("Your YouTube Music sign-in expired. Sign in again.", None);
                for account_id in known {
                    self.emit(Event::YtmSignInExpired { account_id });
                }
            }
            Err((kind, detail)) => {
                self.toast(format!("Couldn't reach YouTube Music: {detail}"), None);
                self.emit(Event::Error {
                    kind,
                    message: "Couldn't reach YouTube Music".into(),
                    detail: Some(detail),
                    account_id: None,
                });
            }
        }
    }

    fn install_youtube_music(
        &mut self,
        account_id: AccountId,
        name: String,
        it: Arc<InnerTube>,
        premium: bool,
        js_host: bool,
        cookies: Option<Secret>,
    ) {
        let provider = Arc::new(YtmProvider::new(account_id.clone(), it, self.clock.clone()));
        provider.set_premium(premium);
        provider.set_name(&name);
        if js_host {
            provider.set_js_host(Some(Arc::new(PlatformJsHost {
                tx: self.tx.clone(),
            })));
        }
        let signed_in = provider.signed_in();
        let priority = self
            .accounts
            .get(&account_id)
            .map(|a| a.info.priority)
            .or_else(|| {
                self.db
                    .accounts()
                    .ok()?
                    .into_iter()
                    .find(|a| a.id == account_id)
                    .map(|a| a.priority)
            })
            .unwrap_or_else(|| self.accounts.next_priority());
        let info = AccountInfo {
            id: account_id.clone(),
            kind: AccountKind::YoutubeMusic,
            priority,
            source: crate::ytm::SOURCE.into(),
            url: "https://music.youtube.com".into(),
            username: if signed_in {
                name.clone()
            } else {
                String::new()
            },
            name: if signed_in {
                format!("YouTube Music ({name})")
            } else {
                "YouTube Music".into()
            },
            capabilities: ServerCapabilities {
                server_version: premium.then(|| "premium".to_string()),
                // The version floor is a Navidrome one: nothing to meet.
                meets_floor: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let sync = signed_in.then(|| {
            let tx = self.tx.clone();
            LibrarySync::for_provider(
                self.db.clone(),
                provider.clone() as Arc<dyn Provider>,
                self.clock.clone(),
                Arc::new(move |p: SyncProgress| {
                    let _ = tx.send(ActorMsg::Internal(Internal::SyncProgress(p)));
                }),
            )
        });
        self.install_provider(info, provider, sync, None, cookies, true);
    }

    pub(crate) fn on_ytm_js(
        &mut self,
        request: YtmJsRequest,
        reply: tokio::sync::oneshot::Sender<Result<YtmJsAnswer, String>>,
    ) {
        let request_id = crate::util::new_id();
        self.ytm_js_pending.retain(|_, r| !r.is_closed());
        self.ytm_js_pending.insert(request_id.clone(), reply);
        self.emit(Event::YtmJsRequest {
            request_id,
            request,
        });
    }

    pub(crate) fn on_ytm_js_answer(
        &mut self,
        request_id: String,
        answer: Option<YtmJsAnswer>,
        error: Option<String>,
    ) {
        if let Some(reply) = self.ytm_js_pending.remove(&request_id) {
            let _ = reply.send(answer.ok_or_else(|| error.unwrap_or_else(|| "no answer".into())));
        }
    }
}
