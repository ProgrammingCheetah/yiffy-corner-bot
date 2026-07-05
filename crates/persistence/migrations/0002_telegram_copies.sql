-- Copy coordinates for channel posts forwarded into the bot as submissions.
-- The bot re-copies the message it saw in the submitter's private chat, so
-- the published post carries content (no forward header) plus a
-- "Forwarded from channel: @…" caption line.
CREATE TABLE telegram_copies (
    source_url        TEXT    PRIMARY KEY,
    origin_chat_id    INTEGER NOT NULL,
    origin_message_id INTEGER NOT NULL,
    channel_username  TEXT    NOT NULL
);
