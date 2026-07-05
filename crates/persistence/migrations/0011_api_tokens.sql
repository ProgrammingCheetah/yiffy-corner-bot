-- Personal API tokens for out-of-Telegram clients (Tampermonkey, curl).
ALTER TABLE users ADD COLUMN api_token TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_api_token ON users(api_token);
