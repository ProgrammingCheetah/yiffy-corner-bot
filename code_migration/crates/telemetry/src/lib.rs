//! The standardized logging vocabulary.
//!
//! Every log line the system emits carries an `event` field drawn from
//! [`Event`], so a JSON log stream can be filtered, counted, and joined on a
//! closed set of machine-readable names (`jq 'select(.event == "published")'`)
//! instead of grepping free-form messages. Secondary classifications
//! ([`RejectReason`], [`SkipReason`], [`Upstream`]) are enums too — the log
//! never invents ad-hoc strings.
//!
//! Convention (documented in `code_migration/README.md`):
//! - `error` — an operation failed; somebody should look.
//! - `warn`  — degraded or suspicious (auth denial, fallback path, empty pool).
//! - `info`  — a domain event happened (submission, decision, publish, …).
//! - `debug` — plumbing detail (upstream requests, candidate walks, DM fan-out).

macro_rules! vocabulary {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self { $($name::$variant => $label),+ })
            }
        }

        impl $name {
            /// Every label in the vocabulary, for docs and tests.
            pub const ALL: &'static [&'static str] = &[$($label),+];
        }
    };
}

vocabulary! {
    /// What happened. One per log line, under the `event` key.
    Event {
        // Boot & lifecycle
        Booting => "booting",
        OwnerSeeded => "owner_seeded",
        RuntimesLoaded => "runtimes_loaded",
        PosterUnbound => "poster_unbound",
        FaCookiesMissing => "fa_cookies_missing",
        HealthServerUp => "health_server_up",
        HealthServerFailed => "health_server_failed",

        // Users & permissions
        UserRegistered => "user_registered",
        DisplayNameRefreshed => "display_name_refreshed",
        AuthDenied => "auth_denied",
        RoleChanged => "role_changed",
        BanChanged => "ban_changed",

        // Submissions
        SubmissionCreated => "submission_created",
        SubmissionRejected => "submission_rejected",
        SubmissionTagsRequested => "submission_tags_requested",
        SubmissionAutoBanned => "submission_auto_banned",
        ForwardRejected => "forward_rejected",
        CopyRefStored => "copy_ref_stored",
        CopyRefStoreFailed => "copy_ref_store_failed",

        // Moderation
        ModerationRequested => "moderation_requested",
        ModerationApplied => "moderation_applied",
        ModerationInvalidState => "moderation_invalid_state",
        PostDeleted => "post_deleted",
        ReviewDmSent => "review_dm_sent",
        ReviewDmFailed => "review_dm_failed",
        CallbackReceived => "callback_received",

        // Tag policy & curation
        TagPolicyChanged => "tag_policy_changed",
        BrowseQueried => "browse_queried",
        BrowseResults => "browse_results",
        BrowseAlbumFailed => "browse_album_failed",
        PoolSaved => "pool_saved",

        // Posters
        PosterCreated => "poster_created",
        PosterTagsChanged => "poster_tags_changed",
        PosterDeleted => "poster_deleted",
        ChannelBound => "channel_bound",

        // Feed & selection
        AcceptedIntoFeed => "accepted_into_feed",
        FeedScanStarted => "feed_scan_started",
        FeedMatch => "feed_match",
        FeedEndReached => "feed_end_reached",
        CursorAdvanced => "cursor_advanced",
        TagsFetched => "tags_fetched",
        StatusFlipped => "status_flipped",
        CandidateSkipped => "candidate_skipped",

        // Scheduler pipeline
        PosterFired => "poster_fired",
        QueuePeekFailed => "queue_peek_failed",
        SelectorFailed => "selector_failed",
        PostSelected => "post_selected",
        MediaResolved => "media_resolved",
        MediaResolveFailed => "media_resolve_failed",
        Published => "published",
        PublishFailed => "publish_failed",
        PublicationRecordFailed => "publication_record_failed",
        MarkPostedFailed => "mark_posted_failed",
        TickFailed => "tick_failed",

        // Reports
        PostReported => "post_reported",
        ReportDuplicate => "report_duplicate",
        ReportNotifyFailed => "report_notify_failed",
        PostTakenDown => "post_taken_down",
        ReportsDismissed => "reports_dismissed",

        // Bot surface
        CommandReceived => "command_received",

        // Upstream plumbing
        UpstreamRequest => "upstream_request",
        UpstreamStatus => "upstream_status",
        MediaLinkFallback => "media_link_fallback",
        FaLoginWall => "fa_login_wall",
    }
}

vocabulary! {
    /// Why a submission was turned away (field `reason` on
    /// `submission_rejected` / `forward_rejected`).
    RejectReason {
        SubmitterBanned => "submitter_banned",
        DuplicateSource => "duplicate_source",
        InvalidSource => "invalid_source",
        PrivateChannel => "private_channel",
    }
}

vocabulary! {
    /// Why the selector passed over a candidate (field `reason` on
    /// `candidate_skipped`).
    SkipReason {
        GlobalForbiddenTag => "global_forbidden_tag",
        SourceUnavailable => "source_unavailable",
        PosterForbiddenTag => "poster_forbidden_tag",
        MissingSubscribedTags => "missing_subscribed_tags",
    }
}

vocabulary! {
    /// Which external service a plumbing event talks about (field `upstream`).
    Upstream {
        E621 => "e621",
        FxTwitter => "fxtwitter",
        Fxbsky => "fxbsky",
        FurAffinity => "furaffinity",
        Telegram => "telegram",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_unique_snake_case() {
        for all in [
            Event::ALL,
            RejectReason::ALL,
            SkipReason::ALL,
            Upstream::ALL,
        ] {
            let mut seen = std::collections::HashSet::new();
            for label in all {
                assert!(seen.insert(label), "duplicate label {label}");
                assert!(
                    label
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "label not snake_case: {label}"
                );
            }
        }
    }

    #[test]
    fn display_matches_label() {
        assert_eq!(Event::Published.to_string(), "published");
        assert_eq!(
            RejectReason::DuplicateSource.to_string(),
            "duplicate_source"
        );
        assert_eq!(
            SkipReason::PosterForbiddenTag.to_string(),
            "poster_forbidden_tag"
        );
        assert_eq!(Upstream::FxTwitter.to_string(), "fxtwitter");
    }
}
