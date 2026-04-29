#!/usr/bin/env bash

set -eu

# Determine script directory (where loop.sh lives, i.e., the loop folder)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Variables - paths relative to script directory
ITERATION_COUNT=100
PROMPT_FILE="${1:-$SCRIPT_DIR/prompt.md}"
LOG_FILE="$SCRIPT_DIR/loop.log"
MCP_CONFIG="${MCP_CONFIG:-$SCRIPT_DIR/mcp.json}"
BREAK_FILE="$SCRIPT_DIR/BREAK"

# Retry configuration for rate limits
MAX_RETRIES=10
INITIAL_WAIT=60      # Start with 1 minute
MAX_WAIT=600         # Cap at 10 minutes

# Token pricing for Opus 4.5 (per token, will be divided by 100 for dollars)
# Input: $5/MTok, Output: $25/MTok, Cache read: 90% discount, Cache write: 25% premium
PRICE_INPUT=0.0005
PRICE_CACHE_READ=0.00005
PRICE_CACHE_WRITE=0.000625
PRICE_OUTPUT=0.0025

# Context window size
CONTEXT_WINDOW=200000

# Color codes
C_RESET='\033[0m'
C_RED='\033[31m'
C_GREEN='\033[32m'
C_YELLOW='\033[33m'
C_BLUE='\033[34m'
C_MAGENTA='\033[35m'
C_CYAN='\033[36m'
C_GRAY='\033[90m'
C_BOLD='\033[1m'

# Cat mascot ASCII art (shared across functions)
CAT1="   ∧＿∧     "
CAT2="  ( ･ω･)    "
CAT3="  |つ　つ   "
CAT4="  |　 |     "
CAT5="   ∪ ∪      "

# Build a context bar string (20 chars wide)
# Sets: CTX_BAR (the bar string), CTX_COLOR (color code for the percentage)
build_context_bar() {
  local pct="${1:-0}" filled i=0
  filled=$((pct / 5))
  CTX_COLOR="$C_GREEN"
  [ "$pct" -ge 50 ] && CTX_COLOR="$C_YELLOW"
  [ "$pct" -ge 80 ] && CTX_COLOR="$C_RED"
  [ "$filled" -gt 20 ] && filled=20
  CTX_BAR=""
  while [ $i -lt $filled ]; do CTX_BAR="${CTX_BAR}█"; i=$((i + 1)); done
  while [ $i -lt 20 ]; do CTX_BAR="${CTX_BAR}░"; i=$((i + 1)); done
}

# Calculate cost from token counts (returns dollars)
# Usage: cost=$(calc_cost $input $cache_read $cache_write $output)
calc_cost() {
  local input="$1" cache_read="$2" cache_write="$3" output="$4"
  echo "scale=2; ($input * $PRICE_INPUT + $cache_read * $PRICE_CACHE_READ + $cache_write * $PRICE_CACHE_WRITE + $output * $PRICE_OUTPUT) / 100" | bc
}

# Calculate metrics summary from cumulative totals
# Sets: CALC_IN_K, CALC_OUT_K, CALC_HIT_PCT, CALC_COST
calc_metrics() {
  local input="$1" output="$2" cache_read="$3" cache_write="$4"
  CALC_IN_K=$(( (input + cache_read) / 1000 ))
  CALC_OUT_K=$(echo "scale=1; $output / 1000" | bc)
  if [ $((input + cache_read)) -gt 0 ]; then
    CALC_HIT_PCT=$(( cache_read * 100 / (input + cache_read) ))
  else
    CALC_HIT_PCT=0
  fi
  CALC_COST=$(calc_cost "$input" "$cache_read" "$cache_write" "$output")
}

# Set terminal/tmux title
set_title() {
  local title="$1"
  if [ -n "${TMUX:-}" ]; then
    # In tmux: set pane title and window name
    printf '\033]2;%s\033\\' "$title"
    printf '\033k%s\033\\' "$title"
  else
    # Standard terminal
    printf '\033]0;%s\007' "$title"
  fi
}

# Trap for both interrupt and normal exit
trap 'printf "\n[Interrupted]\n" >&2; exit 130' INT

# Empty the log file if it exists
echo -n > "$LOG_FILE"

# Remove any stale break signal from previous runs
rm -f "$BREAK_FILE"

# Display configured MCP servers
# Loop runs with --strict-mcp-config: only loop/mcp.json is loaded; global
# MCPs (from `claude mcp add`) and user-scoped configs are ignored.
print_mcp_servers() {
  local has_local=0

  printf "${C_MAGENTA}"
  printf '┌──────────────────────────────────────────────────────────────────────────────┐\n'
  printf '│  🔌 MCP Servers (strict mode — global MCPs ignored)                          │\n'
  printf '├──────────────────────────────────────────────────────────────────────────────┤\n'

  if [ -f "$MCP_CONFIG" ]; then
    local servers
    servers=$(jq -r '.mcpServers | keys[]' "$MCP_CONFIG" 2>/dev/null || true)
    if [ -n "$servers" ]; then
      has_local=1
      printf "│  ${C_BOLD}Loop config (%s):${C_RESET}${C_MAGENTA}%*s│\n" "$MCP_CONFIG" $((54 - ${#MCP_CONFIG})) ""
      echo "$servers" | while IFS= read -r name; do
        local cmd
        cmd=$(jq -r ".mcpServers[\"$name\"].command // \"\"" "$MCP_CONFIG" 2>/dev/null)
        local display="$name"
        [ -n "$cmd" ] && display="$name ($cmd)"
        display=$(echo "$display" | cut -c1-72)
        printf '│    %-72s │\n' "$display"
      done
    fi
  fi

  if [ "$has_local" -eq 0 ]; then
    printf '│  (none — running with zero MCP servers)                                     │\n'
    printf '│  Tip: create %-64s│\n' "$MCP_CONFIG to add loop-scoped MCPs"
  fi

  printf '└──────────────────────────────────────────────────────────────────────────────┘'
  printf "${C_RESET}\n\n"
}

# Print iteration summary by parsing log for usage stats
# Uses line number tracking to only process current iteration's data
LAST_LOG_LINE=0

# Cumulative token tracking
TOTAL_INPUT=0
TOTAL_OUTPUT=0
TOTAL_CACHE_READ=0
TOTAL_CACHE_WRITE=0
CTX_PCT_SUM=0
CTX_PCT_COUNT=0

print_iteration_summary() {
  local iter="$1"
  local total_lines
  total_lines=$(wc -l < "$LOG_FILE" | tr -d ' ')

  # Extract main model from this iteration
  local main_model
  main_model=$(tail -n "+$((LAST_LOG_LINE + 1))" "$LOG_FILE" | { grep '"message_start"' || true; } | jq -rs '
    [.[] | select(.type == "stream_event" and .event.type == "message_start" and .event.message.model)] |
    first | .event.message.model // "unknown"
  ' 2>/dev/null || echo "unknown")

  # Extract subagent stats from this iteration
  # First build a map of tool_use_id -> model from Task invocations
  # Then match with tool results using parent_tool_use_id
  local subagent_stats
  subagent_stats=$(tail -n "+$((LAST_LOG_LINE + 1))" "$LOG_FILE" | { grep -E '"Task"|"agentId"' || true; } | jq -rs '
    # Build map of tool_use_id -> model from Task invocations
    ([.[] | select(.type == "assistant" and .message.content) |
      .message.content[] | select(.type == "tool_use" and .name == "Task") |
      {key: .id, value: (.input.model // "opus")}
    ] | from_entries) as $model_map |
    # Match tool results with their models using message.content[0].tool_use_id
    [.[] | select(.type == "user" and .tool_use_result.agentId)] |
    map({
      id: .tool_use_result.agentId[0:7],
      model: ($model_map[.message.content[0].tool_use_id] // "opus"),
      tokens: ((.tool_use_result.totalTokens // 0) / 1000 | floor),
      duration: ((.tool_use_result.totalDurationMs // 0) / 1000 | floor),
      tools: (.tool_use_result.totalToolUseCount // 0)
    })
  ' 2>/dev/null || echo "[]")

  # Extract lines from this iteration only (from LAST_LOG_LINE to end)
  # Use grep with || true to handle no matches gracefully
  local stats
  stats=$(tail -n "+$((LAST_LOG_LINE + 1))" "$LOG_FILE" | { grep '"message_delta"' || true; } | jq -rs '
    map(select(.type == "stream_event" and .event.type == "message_delta" and .event.usage)) |
    reduce .[] as $item (
      {input: 0, output: 0, cache_read: 0, cache_write: 0};
      .input += ($item.event.usage.input_tokens // 0) |
      .output += ($item.event.usage.output_tokens // 0) |
      .cache_read += ($item.event.usage.cache_read_input_tokens // 0) |
      .cache_write += ($item.event.usage.cache_creation_input_tokens // 0)
    ) |
    "\(.input) \(.output) \(.cache_read) \(.cache_write)"
  ' 2>/dev/null || echo "0 0 0 0")

  # Parse stats into variables
  local iter_input iter_output iter_cache_read iter_cache_write
  iter_input=$(echo "$stats" | cut -d' ' -f1)
  iter_output=$(echo "$stats" | cut -d' ' -f2)
  iter_cache_read=$(echo "$stats" | cut -d' ' -f3)
  iter_cache_write=$(echo "$stats" | cut -d' ' -f4)

  # Update cumulative totals
  TOTAL_INPUT=$((TOTAL_INPUT + iter_input))
  TOTAL_OUTPUT=$((TOTAL_OUTPUT + iter_output))
  TOTAL_CACHE_READ=$((TOTAL_CACHE_READ + iter_cache_read))
  TOTAL_CACHE_WRITE=$((TOTAL_CACHE_WRITE + iter_cache_write))

  # Skip output if no data was processed
  if [ "$iter_input" -eq 0 ] && [ "$iter_output" -eq 0 ] && [ "$iter_cache_read" -eq 0 ]; then
    LAST_LOG_LINE=$total_lines
    return
  fi

  # Calculate iteration metrics
  calc_metrics "$iter_input" "$iter_output" "$iter_cache_read" "$iter_cache_write"
  local in_k="$CALC_IN_K" out_k="$CALC_OUT_K" hit_pct="$CALC_HIT_PCT" cost="$CALC_COST"

  # Get last message's context % from this iteration
  local last_in
  last_in=$(tail -n "+$((LAST_LOG_LINE + 1))" "$LOG_FILE" | { grep '"message_delta"' || true; } | tail -1 | jq -r '
    .event.usage as $u |
    (($u.input_tokens // 0) + ($u.cache_read_input_tokens // 0))
  ' 2>/dev/null || echo "0")
  local iter_ctx_pct=$(( last_in * 100 / CONTEXT_WINDOW ))
  CTX_PCT_SUM=$((CTX_PCT_SUM + iter_ctx_pct))
  CTX_PCT_COUNT=$((CTX_PCT_COUNT + 1))

  # Calculate cumulative metrics
  calc_metrics "$TOTAL_INPUT" "$TOTAL_OUTPUT" "$TOTAL_CACHE_READ" "$TOTAL_CACHE_WRITE"
  local total_in_k="$CALC_IN_K" total_out_k="$CALC_OUT_K" total_hit_pct="$CALC_HIT_PCT" total_cost="$CALC_COST"

  # Print summary with context bar
  build_context_bar "$iter_ctx_pct"

  printf "${C_GREEN}"
  printf '════════════════════════════════════════════════════════════════════════════════\n'
  printf '🤖 Main model: %s\n' "$main_model"
  printf "📊 Context: ${CTX_COLOR}${CTX_BAR}${C_RESET}${C_GREEN} ${CTX_COLOR}%s%%${C_RESET}${C_GREEN} of %sK window\n" "$iter_ctx_pct" "$((CONTEXT_WINDOW / 1000))"
  printf '📈 Iteration %s: %sK in / %sK out | Cache: %s%% hit | Cost: $%s\n' "$iter" "$in_k" "$out_k" "$hit_pct" "$cost"

  # Print subagent stats if any
  local subagent_count
  subagent_count=$(echo "$subagent_stats" | jq -r 'length' 2>/dev/null || echo "0")
  if [ "$subagent_count" -gt 0 ]; then
    printf '├─ Subagents (%s):\n' "$subagent_count"
    echo "$subagent_stats" | jq -r '.[] | "│  • \(.id) (\(.model)): \(.tools) tools, \(.tokens)K tok, \(.duration)s"' 2>/dev/null
  fi

  printf '📊 Total:       %sK in / %sK out | Cache: %s%% hit | Cost: $%s\n' "$total_in_k" "$total_out_k" "$total_hit_pct" "$total_cost"
  printf '════════════════════════════════════════════════════════════════════════════════'
  printf "${C_RESET}\n"

  # Update line counter for next iteration
  LAST_LOG_LINE=$total_lines
}

run_once() {
  local iter="$1"

  # Always strict so global/user-scoped MCPs are ignored.
  # Use loop/mcp.json if present, else an empty config (zero MCPs).
  local mcp_args="--strict-mcp-config"
  if [ -f "$MCP_CONFIG" ]; then
    mcp_args="$mcp_args --mcp-config $MCP_CONFIG"
  else
    mcp_args="$mcp_args --mcp-config {\"mcpServers\":{}}"
  fi

  # Detect tmux for title escape sequences
  local in_tmux=0
  [ -n "${TMUX:-}" ] && in_tmux=1

  # Run claude and tee output to log file and pipe to jq
  cat "$PROMPT_FILE" \
  | claude -p \
    --model opus \
    --verbose \
    --dangerously-skip-permissions \
    --include-partial-messages \
    --output-format=stream-json \
    $mcp_args \
  | tee -a "$LOG_FILE" \
  | jq -rj --unbuffered --argjson iter "$iter" --argjson total "$ITERATION_COUNT" --argjson in_tmux "$in_tmux" \
      --argjson p_in "$PRICE_INPUT" --argjson p_cache_r "$PRICE_CACHE_READ" --argjson p_cache_w "$PRICE_CACHE_WRITE" --argjson p_out "$PRICE_OUTPUT" \
      --argjson ctx_window "$CONTEXT_WINDOW" '
      # Use reduce to maintain state (model_map) across all input lines
      foreach inputs as $line (
        {model_map: {}, output: ""};

        # Process each line and update state
        # Update terminal title with model when we see message_start
        if ($line.type=="stream_event" and $line.event.type=="message_start" and $line.event.message.model) then
          "Loop \($iter)/\($total) | \($line.event.message.model)" as $title |
          if $in_tmux == 1 then
            # tmux: set pane title (\033]2;) and window name (\033k)
            .output = "\u001b]2;\($title)\u001b\\\u001bk\($title)\u001b\\"
          else
            # Standard terminal
            .output = "\u001b]0;\($title)\u0007"
          end

        # Track Task tool invocations with their models
        elif ($line.type=="assistant" and $line.message.content) then
          reduce ($line.message.content[] | select(.type=="tool_use" and .name=="Task")) as $tool
            (.; .model_map[$tool.id] = ($tool.input.model // "opus")) |
          .output = ""

        # Text content
        elif ($line.type=="stream_event" and $line.event.type=="content_block_delta" and $line.event.delta.type=="text_delta") then
          .output = ($line.event.delta.text | gsub("\\r?\\n"; "\n"))

        # Tool start - show with tree branch and iteration prefix (special icon for Task/subagent)
        elif ($line.type=="stream_event" and $line.event.type=="content_block_start" and $line.event.content_block.type=="tool_use") then
          $line.event.content_block.name as $name |
          (now | strftime("%H:%M:%S")) as $ts |
          if $name == "Task" then
            .output = "\n\u001b[36m├─ [\($ts)] 🤖 Agent spawning...\u001b[0m"
          else
            .output = "\n\u001b[33m├─ [\($ts)] 🛠  " + $name + "\u001b[0m"
          end

        # Subagent (Task) result - show summary with model from tracked map
        elif ($line.type=="user" and $line.tool_use_result.agentId?) then
          $line.tool_use_result as $r |
          (now | strftime("%H:%M:%S")) as $ts |
          (($r.totalDurationMs // 0) / 1000) as $secs |
          (($r.totalTokens // 0) / 1000) as $tok_k |
          ($r.totalToolUseCount // 0) as $tools |
          (.model_map[$line.message.content[0].tool_use_id] // "opus") as $model |
          .output = "\n\u001b[36m│  ✓ [\($ts)] Agent " + $r.agentId + " (" + $model + "): " + ($tools|tostring) + " tools, " + ($tok_k|tostring|split(".")[0]) + "K tok, " + ($secs|tostring|split(".")[0]) + "s\u001b[0m\n"

        # Message delta with usage - show stats for this message
        elif ($line.type=="stream_event" and $line.event.type=="message_delta" and $line.event.usage) then
          $line.event.usage as $u |
          (($u.input_tokens // 0) + ($u.cache_read_input_tokens // 0)) as $in |
          ($u.output_tokens // 0) as $out |
          (($in / 1000) | floor) as $in_k |
          ($out / 1000) as $out_k |
          (((($u.input_tokens // 0) * $p_in) + (($u.cache_read_input_tokens // 0) * $p_cache_r) + (($u.cache_creation_input_tokens // 0) * $p_cache_w) + (($u.output_tokens // 0) * $p_out)) / 100) as $cost |
          # Context % based on context window (this message only)
          (($in * 100 / $ctx_window) | floor) as $ctx_pct |
          # Color for context: green < 50%, yellow 50-80%, red > 80%
          (if $ctx_pct >= 80 then "\u001b[31m" elif $ctx_pct >= 50 then "\u001b[33m" else "\u001b[32m" end) as $ctx_color |
          # Build context bar (10 chars wide)
          (($ctx_pct / 10) | floor) as $filled |
          ((10 - $filled) | if . < 0 then 0 else . end) as $empty |
          ($ctx_color + ("█" * $filled) + "\u001b[90m" + ("░" * $empty) + "\u001b[0m") as $bar |
          # Format output with colors
          .output = "\n\u001b[90m└─\u001b[0m \u001b[36m↓" + ($in_k|tostring) + "K\u001b[0m \u001b[33m↑" + ($out_k | tostring | split(".") | if length > 1 then .[0] + "." + (.[1][0:1]) else .[0] end) + "K\u001b[0m " + $bar + " " + $ctx_color + ($ctx_pct|tostring) + "%\u001b[0m \u001b[35m$" + ($cost | tostring | split(".") | if length > 1 then .[0] + "." + (.[1][0:2]) else .[0] end) + "\u001b[0m\n"

        # New text block - add speech bubble with timestamp
        elif ($line.type=="stream_event" and $line.event.type=="content_block_start" and $line.event.content_block.type=="text") then
          (now | strftime("%H:%M:%S")) as $ts |
          .output = "\n\u001b[90m[\($ts)]\u001b[0m 💬 "

        # Skip all other event types
        else
          .output = ""
        end;

        .output
      )'
  printf '\n\n'
}

# Display countdown timer during wait
display_countdown() {
  local remaining="$1"
  while [ "$remaining" -gt 0 ]; do
    printf "\r${C_YELLOW}[Waiting: %02d:%02d remaining]${C_RESET}" $((remaining / 60)) $((remaining % 60))
    sleep 1
    remaining=$((remaining - 1))
  done
  printf '\r\033[K'  # Clear the countdown line
}

# Run with retry logic for rate limits
run_with_retry() {
  local iter="$1" attempt=1 wait_time=$INITIAL_WAIT log_line_before exit_code

  while [ $attempt -le $MAX_RETRIES ]; do
    log_line_before=$(wc -l < "$LOG_FILE" | tr -d ' ')

    set +e
    run_once "$iter"
    exit_code=$?
    set -e

    # Check for rate limit errors in output since this attempt started
    if tail -n "+$((log_line_before + 1))" "$LOG_FILE" | \
       grep -qE '"error".*("overloaded"|"rate_limit"|"capacity"|529|429)|"type":\s*"error".*"overloaded"'; then
      printf "${C_YELLOW}╭─────────────────────────────────────────────────────────────────╮${C_RESET}\n"
      printf "${C_YELLOW}│  ⏸  Rate limited - waiting %3ds before retry %d/%d          │${C_RESET}\n" \
        "$wait_time" "$attempt" "$MAX_RETRIES"
      printf "${C_YELLOW}╰─────────────────────────────────────────────────────────────────╯${C_RESET}\n"

      display_countdown "$wait_time"
      wait_time=$((wait_time * 2))
      [ $wait_time -gt $MAX_WAIT ] && wait_time=$MAX_WAIT
      attempt=$((attempt + 1))
    else
      return $exit_code
    fi
  done

  printf "${C_RED}[Max retries (%d) exceeded - moving to next iteration]${C_RESET}\n" "$MAX_RETRIES"
  return 1
}

# Block digit patterns (5 rows each) - compact format
block_digit() {
  local d="$1" r="$2"
  case "$d$r" in
    01|05) printf "█████" ;; 02|03|04) printf "█   █" ;;
    11|13|14) printf "  █  " ;; 12) printf " ██  " ;; 15) printf "█████" ;;
    21|23|25) printf "█████" ;; 22) printf "    █" ;; 24) printf "█    " ;;
    31|33|35) printf "█████" ;; 32|34) printf "    █" ;;
    41|42) printf "█   █" ;; 43) printf "█████" ;; 44|45) printf "    █" ;;
    51|53|55) printf "█████" ;; 52) printf "█    " ;; 54) printf "    █" ;;
    61|63|65) printf "█████" ;; 62) printf "█    " ;; 64) printf "█   █" ;;
    71) printf "█████" ;; 72) printf "    █" ;; 73) printf "   █ " ;; 74|75) printf "  █  " ;;
    81|83|85) printf "█████" ;; 82|84) printf "█   █" ;;
    91|93|95) printf "█████" ;; 92) printf "█   █" ;; 94) printf "    █" ;;
  esac
}

block_number() {
  local num="$1" row="$2" i=0
  while [ $i -lt ${#num} ]; do
    [ $i -gt 0 ] && printf "  "
    block_digit "${num:$i:1}" "$row"
    i=$((i + 1))
  done
}

print_iteration_header() {
  local n="$1"
  local num_digits=${#n}
  local box_width=$(( num_digits * 5 + (num_digits - 1) * 2 + 4 ))  # digits + spacing + padding

  # Get git info
  local branch="" dirty=""
  if git rev-parse --git-dir >/dev/null 2>&1; then
    branch=$(git branch --show-current 2>/dev/null || git rev-parse --short HEAD 2>/dev/null)
    [ -n "$(git --no-optional-locks status --porcelain 2>/dev/null)" ] && dirty=" *"
  fi

  printf "\n\n${C_GREEN}"
  # Top border
  printf '%s┌' "$CAT1"
  printf '─%.0s' $(seq 1 $box_width)
  printf '┐\n'

  # 5 rows of block number with cat
  local row=1 cats="$CAT2 $CAT3 $CAT4 $CAT5"
  for cat in "$CAT2" "$CAT3" "$CAT4" "$CAT5" "            "; do
    printf '%s│  ' "$cat"
    block_number "$n" "$row"
    printf '  │\n'
    row=$((row + 1))
  done

  # Bottom border
  printf '            └'
  printf '─%.0s' $(seq 1 $box_width)
  printf "┘${C_RESET}\n"

  # Git status line
  if [ -n "$branch" ]; then
    printf "            📁 ${C_BOLD}${C_CYAN}%s${C_RESET} on ${C_BOLD}${C_MAGENTA}%s${C_BOLD}${C_YELLOW}%s${C_RESET}\n" "$(basename "$PWD")" "$branch" "$dirty"
  fi
  printf '\n'
}

print_final_summary() {
  calc_metrics "$TOTAL_INPUT" "$TOTAL_OUTPUT" "$TOTAL_CACHE_READ" "$TOTAL_CACHE_WRITE"
  local total_in_k="$CALC_IN_K" total_out_k="$CALC_OUT_K" total_hit_pct="$CALC_HIT_PCT" total_cost="$CALC_COST"

  # Calculate average context % across all iterations
  local avg_ctx_pct=0
  [ "$CTX_PCT_COUNT" -gt 0 ] && avg_ctx_pct=$((CTX_PCT_SUM / CTX_PCT_COUNT))

  printf "\n${C_CYAN}"
  printf '╔══════════════════════════════════════════════════════════════════════════════╗\n'
  printf '║  FINAL TOTALS: %sK in / %sK out | Cache: %s%% | Avg Ctx: %s%% | Cost: $%s\n' "$total_in_k" "$total_out_k" "$total_hit_pct" "$avg_ctx_pct" "$total_cost"
  printf '╚══════════════════════════════════════════════════════════════════════════════╝'
  printf "${C_RESET}\n"
}

print_done_header() {
  printf '\n\n'
  printf '%s┌────────────────────────────────────┐\n' "$CAT1"
  printf '%s│  ████▄    ████▄  █▄   █  █████▀   │\n' "$CAT2"
  printf '%s│  █    █  █    █  █ █  █  █        │\n' "$CAT3"
  printf '%s│  █    █  █    █  █  █ █  ████     │\n' "$CAT4"
  printf '%s│  █    █  █    █  █   ██  █        │\n' "$CAT5"
  printf '%s│  ████▀    ████▀  █    █  █████▄   │\n' "$CAT5"
  printf '%s└────────────────────────────────────┘\n\n' "$CAT5"
}

# Show MCP servers at startup
print_mcp_servers

i=1
while [ "$i" -le $ITERATION_COUNT ]; do
  # Set terminal title to show current iteration
  set_title "Loop $i/$ITERATION_COUNT"

  print_iteration_header "$i"

  # Add JSON formatted log header with iteration number
  echo "{\"type\":\"loop_start_marker\", \"message\":\"==================== New Claude Run $i - $(date) ====================\"}" >> "$LOG_FILE"

  run_with_retry "$i"

  # Print iteration summary
  print_iteration_summary "$i"

  # Check for break signal from agent
  if [ -f "$BREAK_FILE" ]; then
    printf "\n${C_YELLOW}[Loop terminated: agent signaled completion]${C_RESET}\n"
    rm -f "$BREAK_FILE"
    break
  fi

  # commit and push changes (commented out for safety)
  # git add .
  # git commit -m "Loop iteration $i: $(claude --model haiku -p 'summarize the git changes and create one line git commit message')"
  # git push

  i=$((i+1))
done

echo "{\"type\":\"loop_all_completed\", \"message\":\"==================== Loop complete ====================\"}" >> "$LOG_FILE"

# Final summary and done message
print_final_summary
printf "${C_GREEN}"
print_done_header
printf "${C_RESET}"
