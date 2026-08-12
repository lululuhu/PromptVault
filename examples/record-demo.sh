#!/usr/bin/env bash
# prv demo recording script.
#
# Records a ~30-second terminal demo with asciinema, then converts to GIF.
# Showcases: init → add → commit → diff → log → import → serve.
#
# Usage:
#   1. Install tools:
#        cargo install asciinema
#        cargo install agg
#   2. From the prv repo root, run:
#        bash examples/record-demo.sh
#   3. Output: docs/demo.cast  and  docs/demo.gif
#
# The script drives a fresh throwaway vault in /tmp so it won't touch your real work.

set -euo pipefail

CAST=docs/demo.cast
GIF=docs/demo.gif
mkdir -p docs

# Build the binary once (release for speed, all features for serve + run).
cargo build --release --all-features --quiet
PV="$PWD/target/release/pv"

# A tiny fake ChatGPT export so we can demo `pv import` without a real one.
EXPORT=$(mktemp /tmp/pv-chatgpt-export.XXXXXX.json)
cat > "$EXPORT" <<'JSON'
[
  {
    "title": "Translate to French",
    "create_time": 1700000000,
    "mapping": {
      "root": { "id": "root", "children": ["a"] },
      "a": {
        "id": "a",
        "message": {
          "id": "a",
          "author": { "role": "user" },
          "content": { "content_type": "text", "parts": ["You are a professional translator. Translate the text below into French.\n\n{{text}}"] },
          "create_time": 1700000000
        },
        "parent": "root", "children": []
      }
    }
  }
]
JSON

# asciinema rec runs an interactive shell. We feed it a here-doc of commands.
DRIVER=$(mktemp /tmp/pv-demo-driver.XXXXXX.sh)
cat > "$DRIVER" <<EOF
#!/usr/bin/env bash
export PS1='\$ '
PROMPT_COMMAND=''
slow() { printf '%s\n' "\$1"; sleep "\${2:-0.7}"; }

clear
sleep 0.4
slow '# prv — Prove your prompts' 1.0

# --- init + first prompts ---
slow 'cd /tmp && rm -rf pv-demo && mkdir pv-demo && cd pv-demo' 0.3
slow 'pv init' 0.5
slow 'mkdir prompts && cat > prompts/summarize.md <<EOF
You are a precise summarizer.

Summarize the text below in 3 concise bullets, then propose a title.

{{text}}
EOF' 0.5
slow 'pv add prompts/ && pv commit -m "feat: initial prompt set"' 0.6

# --- iterate + diff ---
slow 'sed -i "s/then propose a title/propose a title, and note the tone/" prompts/summarize.md' 0.3
slow 'pv diff prompts/summarize.md' 1.2
slow 'pv add . && pv commit -m "refine: summarize now reports tone"' 0.5

# --- log ---
slow 'pv log --oneline' 1.0

# --- import from ChatGPT ---
slow 'pv import --from chatgpt $EXPORT --add' 1.0
slow 'pv commit -m "import: chatgpt prompts"' 0.5

# --- serve (Web GUI) ---
slow '# pv serve  →  open http://127.0.0.1:8787  (Web GUI)' 1.2
slow 'echo ✓ done' 0.6
sleep 0.4
exit 0
EOF
chmod +x "$DRIVER"

echo "▶ Recording asciinema cast → $CAST"
asciinema rec "$CAST" --command "$DRIVER" --idle-time-limit 2 --overwrite

rm -f "$DRIVER" "$EXPORT"

echo "▶ Converting to GIF → $GIF"
agg "$CAST" "$GIF" \
  --theme monokai \
  --font-family "JetBrains Mono,Fira Code,monospace" \
  --font-size 14 \
  --speed 1.0 \
  --rows 22

echo ""
echo "✓ Done."
echo "  Cast: $CAST  (upload: asciinema upload $CAST)"
echo "  GIF:  $GIF   (already linked in README)"
