use super::send_discord_activity;

#[test]
fn connects_when_discord_becomes_available() {
    let mut client = None::<usize>;
    let result = send_discord_activity(
        &mut client,
        || Err("Discord offline".into()),
        |_| panic!("Cannot send without a connection"),
    );
    assert!(result.is_err());
    assert!(client.is_none());

    send_discord_activity(
        &mut client,
        || Ok(1),
        |id| {
            assert_eq!(*id, 1);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(client, Some(1));
}

#[test]
fn failed_send_discards_connection_and_next_attempt_reconnects() {
    let mut client = Some(1);
    assert!(send_discord_activity(
        &mut client,
        || panic!("Existing connection must be reused"),
        |_| Err("Disconnected".into()),
    )
    .is_err());
    assert!(client.is_none());

    send_discord_activity(
        &mut client,
        || Ok(2),
        |id| {
            assert_eq!(*id, 2);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(client, Some(2));
}

#[test]
fn healthy_connection_is_reused() {
    let mut client = Some(1);
    send_discord_activity(
        &mut client,
        || panic!("Healthy connection should not reconnect"),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(client, Some(1));
}

#[test]
fn install_outcomes_match_toolbar_protocol() {
    assert_eq!(
        serde_json::to_value(super::UpdateInstallOutcome::NoUpdate).unwrap(),
        "no_update"
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        serde_json::to_value(super::UpdateInstallOutcome::ReleasePage).unwrap(),
        "release_page"
    );
}

#[test]
fn rich_presence_contains_cover_buttons_and_remaining_timer() {
    let end = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 120;
    let presence = super::DiscordPresence {
        details: "Anime".into(),
        state_text: "Серия 5 · Сезон 2".into(),
        poster_url: Some("https://cdn.example/cover.jpg".into()),
        anime_url: Some("https://anivox.fun/anime/123".into()),
        shikimori_url: Some("https://shikimori.io/animes/123".into()),
        end_timestamp: Some(end),
    };
    let json = serde_json::to_value(super::build_discord_activity(&presence)).unwrap();
    assert_eq!(json["type"], 3);
    assert_eq!(json["details"], "Anime");
    assert_eq!(
        json["assets"]["large_image"],
        "https://cdn.example/cover.jpg"
    );
    assert_eq!(json["assets"]["small_image"], "logo");
    assert_eq!(json["timestamps"]["end"], end);
    assert_eq!(json["buttons"].as_array().unwrap().len(), 2);
}

#[test]
fn minimal_presence_omits_timer_buttons_and_falls_back_to_logo() {
    let presence = super::DiscordPresence::default();
    let json = serde_json::to_value(super::build_discord_activity(&presence)).unwrap();
    assert_eq!(json["assets"]["large_image"], "logo");
    assert!(json.get("timestamps").is_none());
    assert!(json.get("buttons").is_none());
}

#[test]
fn invalid_links_and_timestamps_are_not_sent_to_discord() {
    let presence = super::DiscordPresence {
        poster_url: Some("file:///private/cover.jpg".into()),
        anime_url: Some("https://anivox.fun.evil.example/anime/1".into()),
        shikimori_url: Some("https://user:secret@shikimori.io/animes/1".into()),
        end_timestamp: Some(i64::MAX),
        ..Default::default()
    };
    let json = serde_json::to_value(super::build_discord_activity(&presence)).unwrap();
    assert_eq!(json["assets"]["large_image"], "logo");
    assert!(json.get("timestamps").is_none());
    assert!(json.get("buttons").is_none());
}
