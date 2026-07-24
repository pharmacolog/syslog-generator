#!/usr/bin/env bash
# scripts/coordinate.sh — CLI helper для multi-agent coordination protocol.
#
# Реализует lock state machine (см. AGENTS.md §5) и pre-claim checks через
# `gh api` wrappers. НЕ заменяет `gh api graphql` — добавляет удобные shortcuts
# для типичных операций.
#
# Использование:
#   bash scripts/coordinate.sh claim <issue-number>     # claim → Lock State=claimed
#   bash scripts/coordinate.sh active <issue-number>     # active (PR open / commit)
#   bash scripts/coordinate.sh review <issue-number>     # review (PR open)
#   bash scripts/coordinate.sh blocked <issue-number>    # blocked (set Blocker)
#   bash scripts/coordinate.sh released <issue-number>   # released (work done)
#   bash scripts/coordinate.sh status <issue-number>     # show current state
#   bash scripts/coordinate.sh standup "msg"            # post standup to #113
#   bash scripts/coordinate.sh archive-agent <name>      # archive Agent Ops card
#
# Правила:
#   - Все операции idempotent: повторный вызов не создаёт дубли.
#   - Agent name: `OpenCode` (этот скрипт) | `Agent-1` | `Agent-2` | `Maintainer`
#   - Требует GitHub auth (см. AGENTS.md §2). Если `gh auth status` fails — выход 2.
#   - Не делает git operations; только GitHub API.

set -euo pipefail

REPO="pharmacolog/syslog-generator"
OWNER="pharmacolog"

PROJECT_SCRUM="PVT_kwHOACFRws4BePJz"  # Project #2 Scrum Delivery
PROJECT_AGENT_OPS="PVT_kwHOACFRws4BePSR"  # Project #4 Agent Operations
COORD_HUB=113

# Field IDs for Project #4 (Agent Operations).
FIELD_LOCK_STATE="PVTSSF_lAHOACFRws4BePSRzhYq_Y0"
FIELD_AGENT="PVTSSF_lAHOACFRws4BePSRzhYq_UM"
FIELD_SYNC_STATE="PVTSSF_lAHOACFRws4BePSRzhYq_Y4"
FIELD_BRANCH="PVTF_lAHOACFRws4BePSRzhYq_U4"
FIELD_WORKTREE="PVTF_lAHOACFRws4BePSRzhYq_VU"
FIELD_HEARTBEAT="PVTF_lAHOACFRws4BePSRzhYq_ZA"
FIELD_BLOCKED_BY="PVTF_lAHOACFRws4BePSRzhYq_Y8"
FIELD_HANDOFF="PVTF_lAHOACFRws4BePSRzhYq_ZE"
FIELD_FILE_SCOPE="PVTF_lAHOACFRws4BePSRzhYq_XE"

# Lock state option IDs for Project #4.
LOCK_FREE="db782449"
LOCK_CLAIMED="1687158d"
LOCK_ACTIVE="0c146eab"
LOCK_REVIEW="52739b34"
LOCK_BLOCKED="d23c74e9"
LOCK_RELEASED="7b6f10b6"

# Agent option IDs for Project #4.
AGENT_OPENCODE="511871e0"
AGENT_AGENT1="f237237d"
AGENT_AGENT2="1d23157c"
AGENT_MAINTAINER="254efecc"

# Sync state option IDs for Project #4.
SYNC_CURRENT="c297ce6c"
SYNC_BEHIND="4519675b"
SYNC_CONFLICTED="7018f919"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_auth() {
  if ! gh auth status >/dev/null 2>&1; then
    die "gh auth required (см. AGENTS.md §2 GitHub auth bootstrap)"
  fi
}

agent_option_id() {
  case "$1" in
    OpenCode) echo "$AGENT_OPENCODE" ;;
    Agent-1) echo "$AGENT_AGENT1" ;;
    Agent-2) echo "$AGENT_AGENT2" ;;
    Maintainer) echo "$AGENT_MAINTAINER" ;;
    *) die "Unknown agent: $1" ;;
  esac
}

# Find Agent Operations card for an issue number (creates if missing).
find_or_create_card() {
  local issue_number="$1"
  local title="$2"
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | .id')
  if [ -z "$item_id" ] || [ "$item_id" = "null" ]; then
    item_id=$(gh project item-create "$PROJECT_AGENT_OPS" --owner "$OWNER" --title "$title" \
      --body "Coordination session card for Issue #$issue_number. See AGENTS.md §5." \
      --format json | jq -r '.id')
  fi
  echo "$item_id"
}

cmd_claim() {
  local issue_number="$1"
  local title="OpenCode — Issue #${issue_number} (active work)"
  require_auth
  local item_id
  item_id=$(find_or_create_card "$issue_number" "$title")
  local agent_id
  agent_id=$(agent_option_id "OpenCode")
  gh api graphql -f query="mutation { a:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_LOCK_STATE\",value:{singleSelectOptionId:\"$LOCK_CLAIMED\"}}){projectV2Item{id}} b:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_AGENT\",value:{singleSelectOptionId:\"$agent_id\"}}){projectV2Item{id}} c:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_SYNC_STATE\",value:{singleSelectOptionId:\"$SYNC_CURRENT\"}}){projectV2Item{id}} d:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_HEARTBEAT\",value:{date:\"$(date +%Y-%m-%d)\"}}){projectV2Item{id}} }" >/dev/null
  gh issue comment "$issue_number" --repo "$REPO" --body "🤖 OpenCode starting work on this issue" >/dev/null
  gh issue edit "$issue_number" --repo "$REPO" --add-assignee "$OWNER" >/dev/null
  echo "Card $item_id claimed for Issue #$issue_number"
}

cmd_active() {
  local issue_number="$1"
  local title="OpenCode — Issue #${issue_number} (active work)"
  require_auth
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | .id')
  [ -n "$item_id" ] && [ "$item_id" != "null" ] || die "no card found for Issue #$issue_number"
  gh api graphql -f query="mutation { a:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_LOCK_STATE\",value:{singleSelectOptionId:\"$LOCK_ACTIVE\"}}){projectV2Item{id}} b:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_HEARTBEAT\",value:{date:\"$(date +%Y-%m-%d)\"}}){projectV2Item{id}} }" >/dev/null
  echo "Card $item_id set active for Issue #$issue_number"
}

cmd_review() {
  local issue_number="$1"
  local title="OpenCode — Issue #${issue_number} (active work)"
  require_auth
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | .id')
  [ -n "$item_id" ] && [ "$item_id" != "null" ] || die "no card found for Issue #$issue_number"
  gh api graphql -f query="mutation { a:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_LOCK_STATE\",value:{singleSelectOptionId:\"$LOCK_REVIEW\"}}){projectV2Item{id}} b:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_HEARTBEAT\",value:{date:\"$(date +%Y-%m-%d)\"}}){projectV2Item{id}} }" >/dev/null
  echo "Card $item_id set review for Issue #$issue_number"
}

cmd_blocked() {
  local issue_number="$1"
  local reason="$2"
  local title="OpenCode — Issue #${issue_number} (active work)"
  require_auth
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | .id')
  [ -n "$item_id" ] && [ "$item_id" != "null" ] || die "no card found for Issue #$issue_number"
  gh api graphql -f query="mutation { a:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_LOCK_STATE\",value:{singleSelectOptionId:\"$LOCK_BLOCKED\"}}){projectV2Item{id}} b:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_BLOCKED_BY\",value:{text:\"$reason\"}}){projectV2Item{id}} c:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_HEARTBEAT\",value:{date:\"$(date +%Y-%m-%d)\"}}){projectV2Item{id}} }" >/dev/null
  echo "Card $item_id set blocked for Issue #$issue_number: $reason"
}

cmd_released() {
  local issue_number="$1"
  local title="OpenCode — Issue #${issue_number} (active work)"
  require_auth
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | .id')
  [ -n "$item_id" ] && [ "$item_id" != "null" ] || die "no card found for Issue #$issue_number"
  local agent_id
  agent_id=$(agent_option_id "Maintainer")
  gh api graphql -f query="mutation { a:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_LOCK_STATE\",value:{singleSelectOptionId:\"$LOCK_RELEASED\"}}){projectV2Item{id}} b:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_AGENT\",value:{singleSelectOptionId:\"$agent_id\"}}){projectV2Item{id}} c:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_BLOCKED_BY\",value:{text:\"\"}}){projectV2Item{id}} d:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_BRANCH\",value:{text:\"\"}}){projectV2Item{id}} e:updateProjectV2ItemFieldValue(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\",fieldId:\"$FIELD_WORKTREE\",value:{text:\"\"}}){projectV2Item{id}} }" >/dev/null
  echo "Card $item_id set released for Issue #$issue_number"
}

cmd_status() {
  local issue_number="$1"
  local title="OpenCode — Issue #${issue_number} (active work)"
  gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg title "$title" '.items[] | select(.title == $title) | {id, status: (.fieldValues.nodes[] | select(.field.name == "Status") | .name // null), lockState: (.fieldValues.nodes[] | select(.field.name == "Lock State") | .name // null), agent: (.fieldValues.nodes[] | select(.field.name == "Agent") | .name // null), branch: (.fieldValues.nodes[] | select(.field.name == "Branch") | .text // null), heartbeat: (.fieldValues.nodes[] | select(.field.name == "Heartbeat") | .date // null)}'
}

cmd_standup() {
  local msg="$1"
  require_auth
  gh issue comment "$COORD_HUB" --repo "$REPO" --body "$msg" --jq '{id, html_url}'
}

cmd_archive_agent() {
  local name="$1"
  require_auth
  local item_id
  item_id=$(gh project item-list "$PROJECT_AGENT_OPS" --owner "$OWNER" --limit 200 --format json | \
    jq -r --arg name "$name" '.items[] | select(.title == $name) | .id')
  [ -n "$item_id" ] && [ "$item_id" != "null" ] || die "no card found with title: $name"
  gh api graphql -f query="mutation { archiveProjectV2Item(input:{projectId:\"$PROJECT_AGENT_OPS\",itemId:\"$item_id\"}){item{id}} " >/dev/null
  echo "Card $item_id archived"
}

usage() {
  cat <<USAGE
Usage: $0 <command> [args...]

Commands:
  claim <issue#>            Claim Issue (Lock State=claimed, comment, assign)
  active <issue#>           Set Lock State=active (after first commit/push)
  review <issue#>           Set Lock State=review (after PR open)
  blocked <issue#> <reason> Set Lock State=blocked with reason
  released <issue#>         Set Lock State=released, clear branch/worktree
  status <issue#>           Show current state of Agent Ops card
  standup "<msg>"           Post standup comment to Issue #113
  archive-agent <name>      Archive Agent Ops card by title

Examples:
  $0 claim 117
  $0 active 117
  $0 review 117
  $0 blocked 117 "waiting for PR #139 merge"
  $0 released 117
  $0 status 117
  $0 standup "🤖 OpenCode standup 2026-07-24: ..."
  $0 archive-agent "OpenCode — Issue #90 CI/CD hardening"
USAGE
}

if [ $# -lt 1 ]; then
  usage
  exit 2
fi

case "$1" in
  claim)    [ $# -eq 2 ] || { usage; exit 2; }; cmd_claim "$2" ;;
  active)   [ $# -eq 2 ] || { usage; exit 2; }; cmd_active "$2" ;;
  review)   [ $# -eq 2 ] || { usage; exit 2; }; cmd_review "$2" ;;
  blocked)  [ $# -eq 3 ] || { usage; exit 2; }; cmd_blocked "$2" "$3" ;;
  released) [ $# -eq 2 ] || { usage; exit 2; }; cmd_released "$2" ;;
  status)   [ $# -eq 2 ] || { usage; exit 2; }; cmd_status "$2" ;;
  standup)  [ $# -eq 2 ] || { usage; exit 2; }; cmd_standup "$2" ;;
  archive-agent) [ $# -eq 2 ] || { usage; exit 2; }; cmd_archive_agent "$2" ;;
  -h|--help|help) usage ;;
  *) echo "Unknown command: $1" >&2; usage; exit 2 ;;
esac
