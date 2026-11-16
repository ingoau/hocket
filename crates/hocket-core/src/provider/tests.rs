use std::sync::Arc;

use super::*;
use crate::subsonic::fake::FakeServer;

fn provider(sid: &str) -> (FakeServer, Arc<dyn Provider>) {
    let fake = FakeServer::new(sid, "alice");
    fake.add_song(FakeServer::song("t1", "One", "al1", "ar1", 120.0));
    let api: Arc<dyn SubsonicApi> = Arc::new(fake.clone());
    (fake, Arc::new(NavidromeProvider::new(api)))
}

#[tokio::test]
async fn navidrome_answers_in_core_types_and_routes_by_source() {
    let (fake_a, a) = provider("srv-a");
    let (fake_b, b) = provider("srv-b");
    let set = ProviderSet::new(vec![a.clone(), b.clone()]);
    assert_eq!(
        set.sources(),
        vec!["srv-a".to_string(), "srv-b".to_string()]
    );
    let p = set.for_source("srv-b").unwrap();
    assert_eq!(p.kind(), AccountKind::Navidrome);
    let page = p.library_page(0, 10).await.unwrap();
    assert_eq!(page.tracks.len(), 1);
    assert_eq!(page.tracks[0].server_id, "srv-b");
    assert_eq!(page.next_offset, None);

    p.report_play("t1", Some(5.0), true).await.unwrap();
    assert_eq!(fake_b.scrobbles().len(), 1);
    assert!(fake_a.scrobbles().is_empty());

    let s = p
        .resolve_stream(
            "t1",
            &StreamRequest {
                suffix: Some("flac".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(s.url.as_str().contains("stream"), "{s:?}");
    assert!(s.range_capable && !s.transcoded);
    assert_eq!(s.container.as_deref(), Some("flac"));
    assert!(set.subsonic("srv-a").is_some());
    assert!(set.subsonic("ytm").is_none());
}

#[tokio::test]
async fn unsupported_methods_answer_a_capability_error() {
    let (_, p) = provider("srv-a");
    assert!(!p.capabilities().home_shelves);
    match p.home_shelves().await {
        Err(ProviderError::Unsupported(_)) => {}
        other => panic!("{other:?}"),
    }
}
