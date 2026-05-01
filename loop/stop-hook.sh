#!/usr/bin/env bash
# Stop hook for loop.sh.
#
# Fires when the agent finishes its turn. Reads the transcript_path from the
# JSON hook context on stdin, finds the most recent assistant text message,
# and writes loop/BREAK if it contains the sentinel <<<LOOP_DONE>>>.
#
# This is additive insurance for the existing `touch loop/BREAK` mechanism:
# the agent's primary contract still works, and this catches the case where
# the agent ends the loop by sentinel token instead of a Bash tool call.
set -eu

input=$(cat)
transcript=$(printf '%s' "$input" | jq -r '.transcript_path // empty' 2>/dev/null || true)

[ -z "$transcript" ] && exit 0
[ ! -f "$transcript" ] && exit 0

# Pull the last assistant text content from the JSONL transcript.
last_text=$(jq -rs '
  [.[]
    | select(.type == "assistant")
    | (.message.content // [])
    | .[]?
    | select(.type == "text")
    | .text
  ] | last // ""
' "$transcript" 2>/dev/null || true)

if printf '%s' "$last_text" | grep -q "<<<LOOP_DONE>>>"; then
  touch "$(dirname "$0")/BREAK"
fi

exit 0
