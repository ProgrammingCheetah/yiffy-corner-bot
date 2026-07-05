//! SQLite adapter tests, mirroring the in-memory suites, against `:memory:`.

use std::path::PathBuf;

use chrono::{Duration, Utc};
use domain::elements::{
    cadence::PostInterval,
    post::{PostId, PostRepository, PostRepositoryError, PostStatus, Source},
    poster::{PosterId, PosterRepository},
    publisher_config::{PublisherConfig, PublisherConfigRepository},
    tag::Tag,
    tag_policy::{ForbiddenTagRepository, RequiredTagRepository},
    user::{Role, TelegramId, UserId, UserRepository, UserRepositoryError},
};
use url::Url;

use super::{
    post::SqlitePostRepository,
    poster::SqlitePosterRepository,
    publisher_config::SqlitePublisherConfigRepository,
    tag_policy::{SqliteForbiddenTagRepository, SqliteRequiredTagRepository},
    test_pool,
    user::SqliteUserRepository,
};

fn e621_source(id: u64) -> Source {
    Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap()).unwrap()
}

mod users {
    use super::*;

    #[tokio::test]
    async fn create_roundtrip_and_duplicate_rejection() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let user = repo
            .create(
                TelegramId::from(1402476143),
                Role::Owner,
                None,
                Some("Zuri".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(user.role, Role::Owner);
        assert_eq!(user.display_name.as_deref(), Some("Zuri"));
        assert!(!user.is_banned);

        let by_id = repo.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(by_id.telegram_id, user.telegram_id);
        let by_tg = repo
            .find_by_telegram_id(TelegramId::from(1402476143))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_tg.id, user.id);

        let err = repo
            .create(TelegramId::from(1402476143), Role::User, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, UserRepositoryError::NotCreated(_)));
    }

    #[tokio::test]
    async fn change_role_set_display_name_set_banned() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let user = repo
            .create(TelegramId::from(42), Role::User, None, None)
            .await
            .unwrap();

        let promoted = repo.change_role(user.id, Role::Moderator).await.unwrap();
        assert_eq!(promoted.role, Role::Moderator);

        repo.set_display_name(user.id, Some("Ziel".to_string()))
            .await
            .unwrap();
        repo.set_banned(user.id, true).await.unwrap();
        let stored = repo.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(stored.display_name.as_deref(), Some("Ziel"));
        assert!(stored.is_banned);

        repo.set_banned(user.id, false).await.unwrap();
        assert!(!repo.find_by_id(user.id).await.unwrap().unwrap().is_banned);
    }

    #[tokio::test]
    async fn added_by_is_persisted() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let owner = repo
            .create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        let mod_user = repo
            .create(TelegramId::from(2), Role::Moderator, Some(owner.id), None)
            .await
            .unwrap();
        assert_eq!(mod_user.added_by, Some(owner.id));
    }

    #[tokio::test]
    async fn list_by_role_filters() {
        let repo = SqliteUserRepository::new(test_pool().await);
        repo.create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        repo.create(TelegramId::from(2), Role::Moderator, None, None)
            .await
            .unwrap();
        repo.create(TelegramId::from(3), Role::Moderator, None, None)
            .await
            .unwrap();
        assert_eq!(repo.list_by_role(Role::Moderator).await.unwrap().len(), 2);
        assert_eq!(repo.list_by_role(Role::User).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn updates_on_missing_user_error() {
        let repo = SqliteUserRepository::new(test_pool().await);
        assert!(repo.set_banned(UserId::from(99), true).await.is_err());
        assert!(repo.set_display_name(UserId::from(99), None).await.is_err());
        assert!(
            repo.change_role(UserId::from(99), Role::User)
                .await
                .is_err()
        );
    }
}

mod posts {
    use super::*;

    #[tokio::test]
    async fn create_roundtrip_with_submitter_and_status() {
        let pool = test_pool().await;
        let submitter = SqliteUserRepository::new(pool.clone())
            .create(TelegramId::from(7), Role::User, None, None)
            .await
            .unwrap();
        let repo = SqlitePostRepository::new(pool);
        let when = Utc::now();
        let post = repo
            .create(
                e621_source(1),
                vec![],
                vec![],
                Some(submitter.id),
                when,
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        assert_eq!(post.submitted_by, Some(submitter.id));
        assert_eq!(post.status, PostStatus::AwaitingModeration);
        assert!(post.last_posted.is_none());

        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.source, post.source);
        // Timestamps survive the roundtrip to sub-second fidelity.
        assert!((found.submitted_at - when).num_milliseconds().abs() < 1000);
    }

    #[tokio::test]
    async fn find_by_source_and_duplicate_rejection() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let post = repo
            .create(
                e621_source(1),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        let found = repo.find_by_source(&e621_source(1)).await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(post.id));
        assert!(
            repo.find_by_source(&e621_source(2))
                .await
                .unwrap()
                .is_none()
        );

        // UNIQUE(source_url) also guards duplicates at the DB layer.
        assert!(matches!(
            repo.create(
                e621_source(1),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted
            )
            .await
            .unwrap_err(),
            PostRepositoryError::NotCreated(_)
        ));
    }

    #[tokio::test]
    async fn status_transitions_and_mark_posted() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let post = repo
            .create(
                e621_source(1),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();

        repo.set_status_to(post.id, PostStatus::Accepted)
            .await
            .unwrap();
        let at = Utc::now();
        repo.mark_posted(post.id, at).await.unwrap();
        repo.remove(post.id).await.unwrap();

        let stored = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Deleted);
        assert!(stored.last_posted.is_some());
    }

    #[tokio::test]
    async fn missing_post_updates_return_not_found() {
        let repo = SqlitePostRepository::new(test_pool().await);
        assert!(matches!(
            repo.mark_posted(PostId::from(99), Utc::now())
                .await
                .unwrap_err(),
            PostRepositoryError::NotFound(_)
        ));
        assert!(matches!(
            repo.set_status_to(PostId::from(99), PostStatus::Accepted)
                .await
                .unwrap_err(),
            PostRepositoryError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn accept_into_feed_assigns_monotonic_idempotent_positions() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let a = repo
            .create(
                e621_source(1),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        let b = repo
            .create(
                e621_source(2),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        let a = repo.accept_into_feed(a.id).await.unwrap();
        let b = repo.accept_into_feed(b.id).await.unwrap();
        assert_eq!(a.feed_position, Some(1));
        assert_eq!(b.feed_position, Some(2));
        assert_eq!(repo.feed_end().await.unwrap(), 2);

        // Re-accepting a Banned entry keeps its slot.
        repo.set_status_to(a.id, PostStatus::Banned).await.unwrap();
        let again = repo.accept_into_feed(a.id).await.unwrap();
        assert_eq!(again.feed_position, Some(1));
        assert_eq!(again.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn feed_after_windows_orders_and_filters_status() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let mut entries = Vec::new();
        for i in 1..=4u64 {
            let p = repo
                .create(
                    e621_source(i),
                    vec![],
                    vec![],
                    None,
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap();
            entries.push(repo.accept_into_feed(p.id).await.unwrap());
        }
        repo.set_status_to(entries[2].id, PostStatus::Banned)
            .await
            .unwrap();
        repo.remove(entries[3].id).await.unwrap();

        let window = repo.feed_after(1, 4).await.unwrap();
        let positions: Vec<u64> = window.iter().filter_map(|p| p.feed_position).collect();
        assert_eq!(positions, vec![2, 3]);
    }

    #[tokio::test]
    async fn tags_roundtrip_space_joined() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let post = repo
            .create(
                e621_source(1),
                vec![Tag::from("wolf"), Tag::from("male")],
                vec![Tag::from("coolwolf")],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.tags, vec![Tag::from("wolf"), Tag::from("male")]);
        assert_eq!(found.artists, vec![Tag::from("coolwolf")]);
    }

    #[tokio::test]
    async fn poster_cursor_roundtrip() {
        let repo = SqlitePosterRepository::new(test_pool().await);
        let poster = repo
            .create(vec![], vec![], PostInterval::new(5).unwrap())
            .await
            .unwrap();
        assert_eq!(poster.cursor, 0);
        repo.set_cursor(poster.id, 42).await.unwrap();
        assert_eq!(
            repo.find_by_id(poster.id).await.unwrap().unwrap().cursor,
            42
        );
    }

    #[tokio::test]
    async fn list_by_status_orders_oldest_first() {
        let repo = SqlitePostRepository::new(test_pool().await);
        let older = Utc::now() - Duration::hours(2);
        let newer = Utc::now() - Duration::hours(1);
        let b = repo
            .create(
                e621_source(2),
                vec![],
                vec![],
                None,
                newer,
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        let a = repo
            .create(
                e621_source(1),
                vec![],
                vec![],
                None,
                older,
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        repo.create(
            e621_source(3),
            vec![],
            vec![],
            None,
            Utc::now(),
            PostStatus::Accepted,
        )
        .await
        .unwrap();

        let queue = repo
            .list_by_status(PostStatus::AwaitingModeration)
            .await
            .unwrap();
        assert_eq!(
            queue.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![a.id, b.id]
        );
    }
}

mod posters {
    use super::*;

    #[tokio::test]
    async fn create_roundtrip_preserves_tags_and_interval() {
        let repo = SqlitePosterRepository::new(test_pool().await);
        let poster = repo
            .create(
                vec![Tag::from("wolf"), Tag::from("male")],
                vec![Tag::from("gore")],
                PostInterval::new(15).unwrap(),
            )
            .await
            .unwrap();

        let stored = repo.find_by_id(poster.id).await.unwrap().unwrap();
        assert_eq!(
            stored.subscribed_tags,
            vec![Tag::from("wolf"), Tag::from("male")]
        );
        assert_eq!(stored.forbidden_tags, vec![Tag::from("gore")]);
        assert_eq!(stored.time_interval, PostInterval::new(15).unwrap());
    }

    #[tokio::test]
    async fn empty_tag_lists_roundtrip() {
        let repo = SqlitePosterRepository::new(test_pool().await);
        let poster = repo
            .create(vec![], vec![], PostInterval::new(60).unwrap())
            .await
            .unwrap();
        let stored = repo.find_by_id(poster.id).await.unwrap().unwrap();
        assert!(stored.subscribed_tags.is_empty());
        assert!(stored.forbidden_tags.is_empty());
    }

    #[tokio::test]
    async fn set_tags_updates_subscription() {
        let repo = SqlitePosterRepository::new(test_pool().await);
        let poster = repo
            .create(
                vec![Tag::from("fox")],
                vec![],
                PostInterval::new(5).unwrap(),
            )
            .await
            .unwrap();
        let updated = repo
            .set_tags(poster.id, vec![Tag::from("wolf")], vec![Tag::from("gore")])
            .await
            .unwrap();
        assert_eq!(updated.subscribed_tags, vec![Tag::from("wolf")]);
        assert_eq!(updated.forbidden_tags, vec![Tag::from("gore")]);
    }

    #[tokio::test]
    async fn set_rules_roundtrips_through_storage() {
        use domain::elements::tag_rule::TagRule;

        let repo = SqlitePosterRepository::new(test_pool().await);
        let poster = repo
            .create(vec![], vec![], PostInterval::new(5).unwrap())
            .await
            .unwrap();
        assert!(poster.rules.is_empty());

        let rules = TagRule::parse_all("[solo]->[-male] [canine feral]->[cw_feral]").unwrap();
        let updated = repo.set_rules(poster.id, rules.clone()).await.unwrap();
        assert_eq!(updated.rules, rules);

        let stored = repo.find_by_id(poster.id).await.unwrap().unwrap();
        assert_eq!(stored.rules, rules);

        // Empty clears.
        let cleared = repo.set_rules(poster.id, Vec::new()).await.unwrap();
        assert!(cleared.rules.is_empty());
    }

    #[tokio::test]
    async fn list_all_returns_every_poster() {
        let repo = SqlitePosterRepository::new(test_pool().await);
        repo.create(vec![], vec![], PostInterval::new(5).unwrap())
            .await
            .unwrap();
        repo.create(vec![], vec![], PostInterval::new(10).unwrap())
            .await
            .unwrap();
        assert_eq!(repo.list_all().await.unwrap().len(), 2);
    }
}

mod publisher_configs {
    use super::*;

    #[tokio::test]
    async fn upsert_inserts_then_replaces() {
        let pool = test_pool().await;
        let posters = SqlitePosterRepository::new(pool.clone());
        let poster = posters
            .create(vec![], vec![], PostInterval::new(5).unwrap())
            .await
            .unwrap();

        let repo = SqlitePublisherConfigRepository::new(pool);
        repo.upsert(PublisherConfig {
            poster_id: poster.id,
            chat_id: -100,
            token_path: PathBuf::from("a/token.txt"),
            receive_announcements: true,
        })
        .await
        .unwrap();
        // Re-running /setchannel swaps the destination (1:1 invariant).
        repo.upsert(PublisherConfig {
            poster_id: poster.id,
            chat_id: -200,
            token_path: PathBuf::from("b/token.txt"),
            receive_announcements: true,
        })
        .await
        .unwrap();

        let config = repo.find_by_poster(poster.id).await.unwrap().unwrap();
        assert_eq!(config.chat_id, -200);
        assert_eq!(config.token_path, PathBuf::from("b/token.txt"));
        assert_eq!(repo.list_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn announcement_mute_roundtrips_and_survives_rebinding() {
        let pool = test_pool().await;
        let posters = SqlitePosterRepository::new(pool.clone());
        let poster = posters
            .create(vec![], vec![], PostInterval::new(5).unwrap())
            .await
            .unwrap();
        let repo = SqlitePublisherConfigRepository::new(pool);
        repo.upsert(PublisherConfig {
            poster_id: poster.id,
            chat_id: -100,
            token_path: PathBuf::from("a"),
            receive_announcements: true,
        })
        .await
        .unwrap();

        assert_eq!(
            repo.set_receive_announcements(-100, false).await.unwrap(),
            1
        );
        assert!(
            !repo
                .find_by_poster(poster.id)
                .await
                .unwrap()
                .unwrap()
                .receive_announcements
        );

        // Re-binding the same poster keeps the mute.
        repo.upsert(PublisherConfig {
            poster_id: poster.id,
            chat_id: -100,
            token_path: PathBuf::from("b"),
            receive_announcements: true,
        })
        .await
        .unwrap();
        assert!(
            !repo
                .find_by_poster(poster.id)
                .await
                .unwrap()
                .unwrap()
                .receive_announcements
        );

        assert_eq!(
            repo.set_receive_announcements(-999, false).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let repo = SqlitePublisherConfigRepository::new(test_pool().await);
        assert!(
            repo.find_by_poster(PosterId::from(99))
                .await
                .unwrap()
                .is_none()
        );
    }
}

mod tag_policies {
    use super::*;

    #[tokio::test]
    async fn forbidden_add_contains_remove_list() {
        let repo = SqliteForbiddenTagRepository::new(test_pool().await);
        repo.add(Tag::from("gore")).await.unwrap();
        repo.add(Tag::from("gore")).await.unwrap(); // idempotent
        repo.add(Tag::from("scat")).await.unwrap();

        assert!(repo.contains(&Tag::from("gore")).await.unwrap());
        assert!(!repo.contains(&Tag::from("wolf")).await.unwrap());
        assert_eq!(repo.list_all().await.unwrap().len(), 2);

        repo.remove(&Tag::from("gore")).await.unwrap();
        assert!(!repo.contains(&Tag::from("gore")).await.unwrap());
    }

    #[tokio::test]
    async fn required_is_independent_of_forbidden() {
        let pool = test_pool().await;
        let required = SqliteRequiredTagRepository::new(pool.clone());
        let forbidden = SqliteForbiddenTagRepository::new(pool);
        required.add(Tag::from("furry")).await.unwrap();
        assert!(required.contains(&Tag::from("furry")).await.unwrap());
        assert!(!forbidden.contains(&Tag::from("furry")).await.unwrap());
    }
}
