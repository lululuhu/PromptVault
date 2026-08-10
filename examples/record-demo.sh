#!/usr/bin/env bash
# PromptVault demo recording script.
#
# Records a ~40-second terminal demo with asciinema, then converts to GIF.
#
# Usage:
#   1. Install tools:
#        cargo install asciinema
#        cargo install agg
#   2. From the promptvault repo root, run:
#        bash examples/record-demo.sh
#   3. Output: docs/demo.cast  and  docs/demo.gif
#
# The script drives a fresh throwaway vault in /tmp so it won't touch your real work.

set -euo pipefail

CAST=docs/demo.cast
GIF=docs/demo.gif
mkdir -p docs

# Build the binary once (release for speed).
cargo build --release --quiet
PV="$PWD/target/release/pv"

# asciinema rec runs an interactive shell. We feed it a here-doc of commands
# via `expect`-style typing. The simplest portable approach: write a driver
# script and have asciinema execute it.
DRIVER=$(mktemp /tmp/pv-demo-driver.XXXXXX.sh)
cat > "$DRIVER" <<EOF
#!/usr/bin/env bash
# Make the prompt clean and slow enough to read.
export PS1='\$ '
PROMPT_COMMAND=''
# Slow down command echo so the recording is readable.
slow() { printf '%s\n' "\$1"; sleep "\${2:-0.8}"; }

clear
sleep 0.5
slow '# PromptVault — Git for AI Prompts' 1.2

# --- init ---
slow 'cd /tmp && rm -rf pv-demo && mkdir pv-demo && cd pv-demo' 0.4
slow 'pv init' 0.6
PV="$PV"
sleep 0.4

# --- create prompts ---
slow 'mkdir prompts && cat > prompts/summarize.md <<EOF
You are a precise summarizer.

Summarize the text below in 3 concise bullets, then propose a title.

{{text}}
EOF' 0.6
slow 'cat > prompts/code-review.md <<EOF
You are a senior code reviewer.

Review the code below. List bugs, then style issues.

{{code}}
EOF' 0.6
sleep 0.5

# --- add + commit ---
slow 'pv add prompts/' 0.6
sleep 0.4
slow 'pv status' 0.8
sleep 0.4
slow 'pv commit -m "feat: initial prompt set"' 0.8
sleep 0.6

# --- iterate: tweak the prompt, see the diff ---
slow 'sed -i "s/then propose a title/propose a title, and note the tone/" prompts/summarize.md' 0.4
sleep 0.4
slow 'pv diff prompts/summarize.md' 1.4
sleep 0.6

# --- commit the refinement ---
slow 'pv add prompts/summarize.md' 0.4
slow 'pv commit -m "refine: summarize now reports tone"' 0.8
sleep 0.6

# --- log + list ---
slow 'pv log' 1.6
sleep 0.6
slow 'pv list' 1.2
sleep 0.6

# --- branch + A/B ---
slow 'pv branch experiment' 0.6
slow 'pv checkout experiment' 0.8
sleep 0.3
slow 'sed -i "s/3 concise bullets/5 concise bullets/" prompts/summarize.md' 0.4
slow 'pv add . && pv commit -m "experiment: 5 bullets instead of 3"' 0.6
sleep 0.4
slow 'pv checkout main' 0.8
sleep 0.4

# --- tag + show ---
slow 'pv tag v1.0' 0.6
slow 'pv show v1.0' 1.0
sleep 0.6

# --- ref-to-ref diff ---
slow 'pv diff v1.0 experiment' 1.6
sleep 0.8

# --- exit ---
slow 'echo ✓ done' 1.0
sleep 0.6
exit 0
EOF
chmod +x "$DRIVER"

echo "▶ Recording asciinema cast → $CAST"
asciinema rec "$CAST" --command "$DRIVER" --idle-time-limit 2 --overwrite

rm -f "$DRIVER"

echo "▶ Converting to GIF → $GIF"
agg "$CAST" "$GIF" \
  --theme monokai \
  --font-family "JetBrains Mono,Fira Code,monospace" \
  --font-size 14 \
  --speed 1.0 \
  --rows 24

echo ""
echo "✓ Done."
echo "  Cast: $CAST  (upload: asciinema upload $CAST)"
echo "  GIF:  $GIF   (already linked in README)"
