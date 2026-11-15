//! InnerTube client profiles: every name, version and user agent the client
//! sends, in one table so a YouTube change is a one-line bump
//! (`docs/providers.md`, Risks).

use serde_json::{json, Value};

/// Which InnerTube client a request poses as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKind {
    /// The YouTube Music web app: search, browse, library, writes, lyrics.
    /// Its `player` answers need the player JS (signature, `n`) and a PO token.
    WebRemix,
    /// visionOS: plain stream URLs without JS or a PO token (Phase 0).
    VisionOs,
    /// The Quest app: plain URLs, but a PO token is needed past the first MiB.
    AndroidVr,
    /// iOS: often SABR-only (no URLs) anonymously.
    Ios,
    /// The TV app (`TVHTML5`, yt-dlp's `tv_samsung`): takes the signed-in
    /// session and needs no PO token, but its URLs need the player JS
    /// (signature, `n`). The coordinator's way to stream with its account.
    Tv,
}

/// One client profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClientProfile {
    pub kind: ClientKind,
    /// `context.client.clientName`.
    pub name: &'static str,
    pub version: &'static str,
    /// `X-YouTube-Client-Name`.
    pub id: u32,
    pub user_agent: &'static str,
    /// The host its InnerTube calls go to.
    pub host: &'static str,
    pub device_make: Option<&'static str>,
    pub device_model: Option<&'static str>,
    pub os_name: Option<&'static str>,
    pub os_version: Option<&'static str>,
    pub android_sdk: Option<u32>,
    /// Its `player` answers need the platform's JS host (signature, `n`).
    pub needs_js: bool,
    /// Its `player` call carries the signed-in session (cookies and
    /// `SAPISIDHASH`), as the real app does.
    pub session: bool,
}

pub const WEB_REMIX: ClientProfile = ClientProfile {
    kind: ClientKind::WebRemix,
    name: "WEB_REMIX",
    version: "1.20260707.12.00",
    id: 67,
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    host: "music.youtube.com",
    device_make: None,
    device_model: None,
    os_name: None,
    os_version: None,
    android_sdk: None,
    needs_js: true,
    session: true,
};

pub const VISIONOS: ClientProfile = ClientProfile {
    kind: ClientKind::VisionOs,
    name: "VISIONOS",
    version: "1.02",
    id: 101,
    user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    host: "www.youtube.com",
    device_make: Some("Apple"),
    device_model: Some("RealityDevice17,1"),
    os_name: Some("visionOS"),
    os_version: Some("26.5.23O471"),
    android_sdk: None,
    needs_js: false,
    session: false,
};

pub const ANDROID_VR: ClientProfile = ClientProfile {
    kind: ClientKind::AndroidVr,
    name: "ANDROID_VR",
    version: "1.65.10",
    id: 28,
    user_agent: "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
    host: "www.youtube.com",
    device_make: Some("Oculus"),
    device_model: Some("Quest 3"),
    os_name: Some("Android"),
    os_version: Some("12L"),
    android_sdk: Some(32),
    needs_js: false,
    session: false,
};

pub const IOS: ClientProfile = ClientProfile {
    kind: ClientKind::Ios,
    name: "IOS",
    version: "21.26.4",
    id: 5,
    user_agent: "com.google.ios.youtube/21.26.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
    host: "www.youtube.com",
    device_make: Some("Apple"),
    device_model: Some("iPhone16,2"),
    os_name: Some("iPhone"),
    os_version: Some("18.3.2.22D82"),
    android_sdk: None,
    needs_js: false,
    session: false,
};

/// The TV app as yt-dlp's `tv_samsung` poses (its first client for a
/// signed-in session, Nov 2026): `TVHTML5` 5.x accepts cookies, its
/// googlevideo URLs need no PO token (yt-dlp gives it no GVS policy), and
/// the 7.x version is avoided because some sessions get DRM-only formats
/// there (yt-dlp issue 12563). Its URLs are ciphered: signature and `n` come
/// from the main player JS (`player_ias.vflset/en_US/base.js`).
pub const TV: ClientProfile = ClientProfile {
    kind: ClientKind::Tv,
    name: "TVHTML5",
    version: "5.20260707",
    id: 7,
    user_agent: "Mozilla/5.0 (SMART-TV; Linux; Tizen 2.4.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/2.4.0 TV Safari/538.1",
    host: "www.youtube.com",
    device_make: Some("Samsung"),
    device_model: Some("UKS9800"),
    os_name: Some("Tizen"),
    os_version: Some("2.4.0"),
    android_sdk: None,
    needs_js: true,
    session: true,
};

/// The `player` fallback chain, in order (`docs/providers.md`, Decisions).
/// The JS clients take part only when the JS host offers them
/// ([`super::player::JsHost::clients`]); a signed-in session moves the
/// session-carrying TV client to the front.
pub const PLAYER_CHAIN: [ClientProfile; 5] = [VISIONOS, ANDROID_VR, IOS, WEB_REMIX, TV];

impl ClientProfile {
    pub fn by_kind(kind: ClientKind) -> ClientProfile {
        match kind {
            ClientKind::WebRemix => WEB_REMIX,
            ClientKind::VisionOs => VISIONOS,
            ClientKind::AndroidVr => ANDROID_VR,
            ClientKind::Ios => IOS,
            ClientKind::Tv => TV,
        }
    }

    /// `context.client` for a request.
    pub fn context(&self, visitor_data: Option<&str>, hl: &str, gl: &str) -> Value {
        let mut c = json!({
            "clientName": self.name,
            "clientVersion": self.version,
            "hl": hl,
            "gl": gl,
        });
        let m = c.as_object_mut().expect("object");
        if let Some(v) = visitor_data {
            m.insert("visitorData".into(), v.into());
        }
        if let Some(v) = self.device_make {
            m.insert("deviceMake".into(), v.into());
        }
        if let Some(v) = self.device_model {
            m.insert("deviceModel".into(), v.into());
        }
        if let Some(v) = self.os_name {
            m.insert("osName".into(), v.into());
        }
        if let Some(v) = self.os_version {
            m.insert("osVersion".into(), v.into());
        }
        if let Some(v) = self.android_sdk {
            m.insert("androidSdkVersion".into(), v.into());
        }
        json!({ "client": c })
    }
}
