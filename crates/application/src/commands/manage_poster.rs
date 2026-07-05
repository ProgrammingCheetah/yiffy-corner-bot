//! `/newposter` and `/setchannel` — Owner-only Poster lifecycle.
//!
//! Per design, only Zuri creates Posters. A Poster is born from its tag
//! subscription + cadence; `/setchannel` then binds it to a delivery
//! destination by upserting its [`PublisherConfig`] (re-running swaps the
//! destination rather than erroring — the 1:1 invariant).

use std::path::PathBuf;

use domain::elements::{
    cadence::PostInterval,
    poster::{Poster, PosterId, PosterRepository},
    publisher_config::{PublisherConfig, PublisherConfigRepository},
    tag::Tag,
    user::{Role, TelegramId, UserRepository},
};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::Event;

#[derive(Debug)]
pub struct NewPoster {
    pub actor: TelegramId,
    pub subscribed_tags: Vec<Tag>,
    pub forbidden_tags: Vec<Tag>,
    pub interval: PostInterval,
    /// The delivery destination — a poster is born bound (QoL: one command
    /// instead of /newposter + /setchannel).
    pub chat_id: i64,
    pub token_path: PathBuf,
}

pub async fn new_poster<P, C>(
    cmd: NewPoster,
    users: &impl UserRepository,
    posters: &P,
    configs: &C,
) -> HandlerResult<Poster>
where
    P: PosterRepository,
    C: PublisherConfigRepository,
{
    require_role(users, cmd.actor, Role::Owner).await?;
    let poster = posters
        .create(cmd.subscribed_tags, cmd.forbidden_tags, cmd.interval)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    configs
        .upsert(PublisherConfig {
            poster_id: poster.id,
            chat_id: cmd.chat_id,
            token_path: cmd.token_path,
            receive_announcements: true,
        })
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::PosterCreated,
        poster_id = %poster.id,
        interval_min = poster.time_interval.as_ref(),
        subscribed = ?poster.subscribed_tags,
        forbidden = ?poster.forbidden_tags,
        chat_id = cmd.chat_id,
        "poster created and bound"
    );
    Ok(poster)
}

#[derive(Debug)]
pub struct SetTags {
    pub actor: TelegramId,
    pub poster_id: PosterId,
    pub subscribed_tags: Vec<Tag>,
    pub forbidden_tags: Vec<Tag>,
}

/// Replace a Poster's tag subscription. Owner-only.
pub async fn set_tags<P>(
    cmd: SetTags,
    users: &impl UserRepository,
    posters: &P,
) -> HandlerResult<Poster>
where
    P: PosterRepository,
{
    require_role(users, cmd.actor, Role::Owner).await?;
    let poster = posters
        .set_tags(cmd.poster_id, cmd.subscribed_tags, cmd.forbidden_tags)
        .await
        .map_err(|_| {
            HandlerError::InvalidState(format!("poster {} does not exist", cmd.poster_id))
        })?;
    tracing::info!(
        event = %Event::PosterTagsChanged,
        poster_id = %poster.id,
        subscribed = ?poster.subscribed_tags,
        forbidden = ?poster.forbidden_tags,
        "poster tag subscription replaced"
    );
    Ok(poster)
}

/// Replace a Poster's conditional tag rules. Owner-only; live next tick.
pub async fn set_rules<P>(
    actor: TelegramId,
    poster_id: PosterId,
    rules: Vec<domain::elements::tag_rule::TagRule>,
    users: &impl UserRepository,
    posters: &P,
) -> HandlerResult<Poster>
where
    P: PosterRepository,
{
    require_role(users, actor, Role::Owner).await?;
    let poster = posters
        .set_rules(poster_id, rules)
        .await
        .map_err(|_| HandlerError::InvalidState(format!("poster {poster_id} does not exist")))?;
    tracing::info!(
        event = %Event::PosterRulesChanged,
        poster_id = %poster.id,
        rules = %poster
            .rules
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        "conditional rules replaced"
    );
    Ok(poster)
}

/// Change a Poster's cadence. Owner-only; live on the next tick.
pub async fn set_interval<P>(
    actor: TelegramId,
    poster_id: PosterId,
    interval: PostInterval,
    users: &impl UserRepository,
    posters: &P,
) -> HandlerResult<Poster>
where
    P: PosterRepository,
{
    require_role(users, actor, Role::Owner).await?;
    let poster = posters
        .set_interval(poster_id, interval)
        .await
        .map_err(|_| HandlerError::InvalidState(format!("poster {poster_id} does not exist")))?;
    tracing::info!(
        event = %Event::PosterIntervalChanged,
        poster_id = %poster.id,
        interval_min = poster.time_interval.as_ref(),
        "poster cadence changed"
    );
    Ok(poster)
}

/// Mute or unmute announcement delivery for a chat (Owner-only). The chat
/// keeps appearing in directories broadcast to other channels.
pub async fn set_announcement_mute<C>(
    actor: TelegramId,
    chat_id: i64,
    muted: bool,
    users: &impl UserRepository,
    configs: &C,
) -> HandlerResult<u64>
where
    C: PublisherConfigRepository,
{
    require_role(users, actor, Role::Owner).await?;
    let affected = configs
        .set_receive_announcements(chat_id, !muted)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    if affected == 0 {
        return Err(HandlerError::InvalidState(format!(
            "no poster is bound to chat {chat_id}"
        )));
    }
    tracing::info!(
        event = %Event::AnnouncementConfigChanged,
        chat_id, muted, affected, "announcement delivery toggled"
    );
    Ok(affected)
}

/// Delete a Poster (and its channel binding). Owner-only. The feed and its
/// posts are untouched — only this consumer disappears; the database-first
/// scheduler stops firing it on the next tick.
pub async fn delete_poster<P, C>(
    actor: TelegramId,
    poster_id: PosterId,
    users: &impl UserRepository,
    posters: &P,
    configs: &C,
) -> HandlerResult<()>
where
    P: PosterRepository,
    C: PublisherConfigRepository,
{
    require_role(users, actor, Role::Owner).await?;
    posters
        .find_by_id(poster_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or_else(|| HandlerError::InvalidState(format!("poster {poster_id} does not exist")))?;
    // Binding first: publisher_configs holds the FK onto posters.
    configs
        .remove(poster_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    posters
        .delete(poster_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(event = %Event::PosterDeleted, poster_id = %poster_id, "poster deleted");
    Ok(())
}

#[derive(Debug)]
pub struct SetChannel {
    pub actor: TelegramId,
    pub poster_id: PosterId,
    pub chat_id: i64,
    pub token_path: PathBuf,
}

pub async fn set_channel<P, C>(
    cmd: SetChannel,
    users: &impl UserRepository,
    posters: &P,
    configs: &C,
) -> HandlerResult<()>
where
    P: PosterRepository,
    C: PublisherConfigRepository,
{
    require_role(users, cmd.actor, Role::Owner).await?;
    posters
        .find_by_id(cmd.poster_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or_else(|| {
            HandlerError::InvalidState(format!("poster {} does not exist", cmd.poster_id))
        })?;
    tracing::info!(event = %Event::ChannelBound, poster_id = %cmd.poster_id, chat_id = cmd.chat_id, "poster channel binding upserted");
    configs
        .upsert(PublisherConfig {
            poster_id: cmd.poster_id,
            chat_id: cmd.chat_id,
            token_path: cmd.token_path,
            receive_announcements: true,
        })
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

#[cfg(test)]
mod tests {
    use super::*;

    use persistence::in_memory::{
        poster::InMemoryPosterRepository, publisher_config::InMemoryPublisherConfigRepository,
        user::InMemoryUserRepository,
    };

    struct Fixture {
        users: InMemoryUserRepository,
        posters: InMemoryPosterRepository,
        configs: InMemoryPublisherConfigRepository,
    }

    async fn fixture() -> Fixture {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::Moderator, None, None)
            .await
            .unwrap();
        Fixture {
            users,
            posters: InMemoryPosterRepository::new(),
            configs: InMemoryPublisherConfigRepository::new(),
        }
    }

    fn new_poster_cmd(actor: i64) -> NewPoster {
        NewPoster {
            actor: TelegramId::from(actor),
            subscribed_tags: vec![Tag::from("wolf")],
            forbidden_tags: vec![Tag::from("gore")],
            interval: PostInterval::new(15).unwrap(),
            chat_id: -100555,
            token_path: PathBuf::from("config/vault/dev/token.txt"),
        }
    }

    #[tokio::test]
    async fn owner_creates_poster() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        assert_eq!(poster.subscribed_tags, vec![Tag::from("wolf")]);
        let stored = fx.posters.find_by_id(poster.id).await.unwrap();
        assert!(stored.is_some());
        // Born bound: the binding was created alongside.
        let config = fx.configs.find_by_poster(poster.id).await.unwrap().unwrap();
        assert_eq!(config.chat_id, -100555);
    }

    #[tokio::test]
    async fn moderator_cannot_create_poster() {
        let fx = fixture().await;
        let err = new_poster(new_poster_cmd(2), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn owner_replaces_poster_tags() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        let updated = set_tags(
            SetTags {
                actor: TelegramId::from(1),
                poster_id: poster.id,
                subscribed_tags: vec![Tag::from("dragon")],
                forbidden_tags: vec![],
            },
            &fx.users,
            &fx.posters,
        )
        .await
        .unwrap();
        assert_eq!(updated.subscribed_tags, vec![Tag::from("dragon")]);
    }

    #[tokio::test]
    async fn moderator_cannot_replace_poster_tags() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        let err = set_tags(
            SetTags {
                actor: TelegramId::from(2),
                poster_id: poster.id,
                subscribed_tags: vec![],
                forbidden_tags: vec![],
            },
            &fx.users,
            &fx.posters,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn set_channel_binds_existing_poster() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        set_channel(
            SetChannel {
                actor: TelegramId::from(1),
                poster_id: poster.id,
                chat_id: -100123,
                token_path: PathBuf::from("config/vault/dev/posters/1/token.txt"),
            },
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap();

        let config = fx.configs.find_by_poster(poster.id).await.unwrap().unwrap();
        assert_eq!(config.chat_id, -100123);
    }

    #[tokio::test]
    async fn owner_deletes_poster_and_binding() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        set_channel(
            SetChannel {
                actor: TelegramId::from(1),
                poster_id: poster.id,
                chat_id: -100,
                token_path: PathBuf::from("t"),
            },
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap();

        delete_poster(
            TelegramId::from(1),
            poster.id,
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap();
        assert!(fx.posters.find_by_id(poster.id).await.unwrap().is_none());
        assert!(
            fx.configs
                .find_by_poster(poster.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn moderator_cannot_delete_poster() {
        let fx = fixture().await;
        let poster = new_poster(new_poster_cmd(1), &fx.users, &fx.posters, &fx.configs)
            .await
            .unwrap();
        let err = delete_poster(
            TelegramId::from(2),
            poster.id,
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn deleting_unknown_poster_is_invalid_state() {
        let fx = fixture().await;
        let err = delete_poster(
            TelegramId::from(1),
            PosterId::from(99),
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn set_channel_rejects_unknown_poster() {
        let fx = fixture().await;
        let err = set_channel(
            SetChannel {
                actor: TelegramId::from(1),
                poster_id: PosterId::from(999),
                chat_id: -100123,
                token_path: PathBuf::from("x"),
            },
            &fx.users,
            &fx.posters,
            &fx.configs,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }
}
