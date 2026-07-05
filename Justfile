# Yiffy Corner bot — operations.
#
#   just setup            interactive vault provisioning (production)
#   just setup dev        …for the development vault instead
#   just start            build + run the bot (docker compose, detached)
#   just stop             stop the bot
#   just logs             follow the JSON log
#   just status           container + health check

default:
    @just --list

# Provision config/vault/<env>/ interactively (blank = keep, secrets unechoed)
setup env="production":
    #!/usr/bin/env bash
    set -euo pipefail
    vault="config/vault/{{ env }}"
    mkdir -p "$vault" config/vault/storage
    echo "Provisioning $vault (blank answer = keep current value)"
    echo

    ask() { # ask <file> <label> <required:yes|no>
        local file="$vault/$1" label="$2" required="$3" current="absent" value
        [ -s "$file" ] && current="set"
        while true; do
            read -rsp "$label [$current]: " value; echo
            if [ -n "$value" ]; then
                printf '%s' "$value" > "$file"
                echo "  → written"
                return
            fi
            if [ "$current" = "set" ] || [ "$required" = "no" ]; then
                echo "  → kept"
                return
            fi
            echo "  required — please enter a value"
        done
    }

    ask token.txt       "Telegram bot token (required)"        yes
    ask e621_login.txt  "e621 username (optional)"             no
    ask e621_key.txt    "e621 API key (optional)"              no
    ask cookie_a.txt    "FurAffinity cookie 'a' (optional)"    no
    ask cookie_b.txt    "FurAffinity cookie 'b' (optional)"    no

    chmod 600 "$vault"/*.txt 2>/dev/null || true
    echo
    echo "Vault ready. Start the bot with: just start"

# Build + run the bot detached (YCB_ENV=production unless overridden)
start:
    docker compose up -d --build bot-rust
    @echo "Bot starting — follow with: just logs"

# Stop the bot container
stop:
    docker compose stop bot-rust

# Follow the JSON log stream
logs:
    docker logs -f yiffy_corner_bot_rust

# Container state + health endpoint
status:
    @docker compose ps bot-rust
    @curl -sf -o /dev/null -w 'health: %{http_code}\n' http://127.0.0.1:3000/health \
        || echo 'health: unreachable'
