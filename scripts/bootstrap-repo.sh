#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/bootstrap-repo.sh OWNER REPO
  scripts/bootstrap-repo.sh OWNER/REPO

Environment:
  GH_API_VERSION            GitHub REST API version header.
  ALLOWED_ACTION_PATTERNS   Comma-separated selected-actions allowlist.

Requires:
  gh authenticated with repository administration access.
  jq.
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

json_tmp() {
  mktemp "${TMPDIR:-/tmp}/bootstrap-repo.XXXXXX.json"
}

gh_api() {
  gh api -H "X-GitHub-Api-Version: ${GH_API_VERSION}" "$@"
}

urlencode() {
  jq -nr --arg value "$1" '$value | @uri'
}

if [ "$#" -eq 1 ]; then
  case "$1" in
    */*)
      OWNER=${1%%/*}
      REPO=${1#*/}
      ;;
    *)
      usage >&2
      die "single-argument form must be OWNER/REPO"
      ;;
  esac
elif [ "$#" -eq 2 ]; then
  OWNER=$1
  REPO=$2
else
  usage >&2
  exit 2
fi

[ -n "$OWNER" ] || die "owner is required"
[ -n "$REPO" ] || die "repo is required"

GH_API_VERSION=${GH_API_VERSION:-2022-11-28}
SCRIPT_DIR=$(unset CDPATH; cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(unset CDPATH; cd -- "$SCRIPT_DIR/.." && pwd)
RULESET_DIR=$REPO_ROOT/.github/rulesets

DEFAULT_ALLOWED_ACTION_PATTERNS='anthropics/claude-code-action@*,aws-actions/amazon-ecr-login@*,aws-actions/configure-aws-credentials@*,docker/build-push-action@*,docker/setup-buildx-action@*,openai/codex-action@*,ossf/scorecard-action@*,step-security/harden-runner@*,trufflesecurity/trufflehog@*,aquasecurity/trivy-action@*,bridgecrewio/checkov-action@*,zgosalvez/github-actions-ensure-sha-pinned-actions@*,opentofu/setup-opentofu@*,dtolnay/rust-toolchain@*,Swatinem/rust-cache@*,bencherdev/bencher@*,google-github-actions/auth@*,google-github-actions/setup-gcloud@*,pnpm/action-setup@*,astral-sh/setup-uv@*,softprops/action-gh-release@*,actions/setup-python@*,actions/cache@*,actions/download-artifact@*'
ALLOWED_ACTION_PATTERNS=${ALLOWED_ACTION_PATTERNS:-$DEFAULT_ALLOWED_ACTION_PATTERNS}

need gh
need jq

printf 'Bootstrapping repository security settings for %s/%s\n' "$OWNER" "$REPO"

# Requires repo Administration:write. Organization-level security defaults and
# organization Actions policies are org-admin calls and are intentionally not
# changed by this repository bootstrap script.
repo_settings=$(json_tmp)
jq -n '{
  delete_branch_on_merge: true,
  allow_squash_merge: true,
  allow_merge_commit: false,
  allow_rebase_merge: false
}' >"$repo_settings"
gh_api --method PATCH "/repos/$OWNER/$REPO" --input "$repo_settings" >/dev/null
rm -f "$repo_settings"

workflow_permissions=$(json_tmp)
jq -n '{
  default_workflow_permissions: "read",
  can_approve_pull_request_reviews: false
}' >"$workflow_permissions"
gh_api --method PUT "/repos/$OWNER/$REPO/actions/permissions/workflow" --input "$workflow_permissions" >/dev/null
rm -f "$workflow_permissions"

actions_permissions=$(json_tmp)
jq -n '{
  enabled: true,
  allowed_actions: "selected",
  sha_pinning_required: true
}' >"$actions_permissions"
if ! gh_api --method PUT "/repos/$OWNER/$REPO/actions/permissions" --input "$actions_permissions" >/dev/null 2>&1; then
  printf 'GitHub API did not accept sha_pinning_required; retrying allowed_actions without that field.\n' >&2
  jq 'del(.sha_pinning_required)' "$actions_permissions" >"$actions_permissions.no-sha"
  gh_api --method PUT "/repos/$OWNER/$REPO/actions/permissions" --input "$actions_permissions.no-sha" >/dev/null
  rm -f "$actions_permissions.no-sha"
fi
rm -f "$actions_permissions"

selected_actions=$(json_tmp)
jq -n --arg patterns "$ALLOWED_ACTION_PATTERNS" '{
  github_owned_allowed: true,
  verified_allowed: true,
  patterns_allowed: (
    $patterns
    | split(",")
    | map(gsub("^\\s+|\\s+$"; ""))
    | map(select(length > 0))
  )
}' >"$selected_actions"
gh_api --method PUT "/repos/$OWNER/$REPO/actions/permissions/selected-actions" --input "$selected_actions" >/dev/null
rm -f "$selected_actions"

gh_api --method PUT "/repos/$OWNER/$REPO/vulnerability-alerts" >/dev/null
gh_api --method PUT "/repos/$OWNER/$REPO/automated-security-fixes" >/dev/null

gh_api --method PUT "/repos/$OWNER/$REPO/immutable-releases" >/dev/null

apply_ruleset() {
  ruleset_file=$1
  ruleset_name=$(jq -r '.name' "$ruleset_file")
  [ "$ruleset_name" != "null" ] && [ -n "$ruleset_name" ] || die "ruleset name missing in $ruleset_file"
  api_ruleset_file=$(json_tmp)

  # GitHub's REST ruleset API currently rejects the Copilot review toggle in
  # repository rulesets. Keep the committed JSON explicit, but strip that field
  # from the request body so this setup script remains idempotent in real repos.
  jq '
    (.rules[] | select(.type == "pull_request") | .parameters) |=
      del(.automatic_copilot_code_review_enabled)
  ' "$ruleset_file" >"$api_ruleset_file"

  ruleset_id=$(
    gh_api "/repos/$OWNER/$REPO/rulesets?includes_parents=false&per_page=100" \
      --jq ".[] | select(.source_type == \"Repository\" and .name == \"$ruleset_name\") | .id" \
      | head -n 1
  )

  if [ -n "$ruleset_id" ]; then
    gh_api --method PUT "/repos/$OWNER/$REPO/rulesets/$ruleset_id" --input "$api_ruleset_file" >/dev/null
    printf 'Updated ruleset: %s\n' "$ruleset_name"
  else
    gh_api --method POST "/repos/$OWNER/$REPO/rulesets" --input "$api_ruleset_file" >/dev/null
    printf 'Created ruleset: %s\n' "$ruleset_name"
  fi

  rm -f "$api_ruleset_file"
}

[ -d "$RULESET_DIR" ] || die "ruleset directory not found: $RULESET_DIR"
for ruleset_file in "$RULESET_DIR"/*.json; do
  [ -e "$ruleset_file" ] || die "no ruleset JSON files found in $RULESET_DIR"
  apply_ruleset "$ruleset_file"
done

put_environment() {
  env_name=$1
  encoded_env=$(urlencode "$env_name")
  env_body=$(json_tmp)
  jq -n '{
    deployment_branch_policy: {
      protected_branches: false,
      custom_branch_policies: true
    }
  }' >"$env_body"
  gh_api --method PUT "/repos/$OWNER/$REPO/environments/$encoded_env" --input "$env_body" >/dev/null
  rm -f "$env_body"
}

ensure_deployment_policy() {
  env_name=$1
  pattern=$2
  policy_type=$3
  encoded_env=$(urlencode "$env_name")
  policy_json=$(
    gh_api "/repos/$OWNER/$REPO/environments/$encoded_env/deployment-branch-policies?per_page=100" \
      --jq ".branch_policies[]? | select(.name == \"$pattern\") | {id, type}" \
      | head -n 1
  )

  if [ -n "$policy_json" ]; then
    existing_id=$(printf '%s\n' "$policy_json" | jq -r '.id')
    existing_type=$(printf '%s\n' "$policy_json" | jq -r '.type')
    if [ "$existing_type" = "$policy_type" ]; then
      return 0
    fi
    gh_api --method DELETE "/repos/$OWNER/$REPO/environments/$encoded_env/deployment-branch-policies/$existing_id" >/dev/null
  fi

  policy_body=$(json_tmp)
  jq -n --arg name "$pattern" --arg type "$policy_type" '{name: $name, type: $type}' >"$policy_body"
  gh_api --method POST "/repos/$OWNER/$REPO/environments/$encoded_env/deployment-branch-policies" --input "$policy_body" >/dev/null
  rm -f "$policy_body"
}

sync_deployment_policies() {
  env_name=$1
  shift
  encoded_env=$(urlencode "$env_name")
  policies_file=$(json_tmp)

  gh_api "/repos/$OWNER/$REPO/environments/$encoded_env/deployment-branch-policies?per_page=100" >"$policies_file"

  while IFS= read -r policy; do
    policy_id=$(printf '%s\n' "$policy" | jq -r '.id')
    policy_name=$(printf '%s\n' "$policy" | jq -r '.name')
    policy_type=$(printf '%s\n' "$policy" | jq -r '.type')
    keep_policy=false

    for desired_policy in "$@"; do
      desired_type=${desired_policy%%:*}
      desired_name=${desired_policy#*:}
      if [ "$policy_type" = "$desired_type" ] && [ "$policy_name" = "$desired_name" ]; then
        keep_policy=true
        break
      fi
    done

    if [ "$keep_policy" = false ]; then
      gh_api --method DELETE "/repos/$OWNER/$REPO/environments/$encoded_env/deployment-branch-policies/$policy_id" >/dev/null
      printf 'Removed stale deployment policy from %s: %s %s\n' "$env_name" "$policy_type" "$policy_name"
    fi
  done < <(jq -c '.branch_policies[]? | {id, name, type}' "$policies_file")

  rm -f "$policies_file"

  for desired_policy in "$@"; do
    desired_type=${desired_policy%%:*}
    desired_name=${desired_policy#*:}
    ensure_deployment_policy "$env_name" "$desired_name" "$desired_type"
  done
}

ensure_label() {
  label_name=$1
  label_color=$2
  label_description=$3
  encoded_label=$(urlencode "$label_name")
  label_body=$(json_tmp)

  jq -n \
    --arg name "$label_name" \
    --arg color "$label_color" \
    --arg description "$label_description" \
    '{name: $name, color: $color, description: $description}' >"$label_body"

  if gh_api "/repos/$OWNER/$REPO/labels/$encoded_label" >/dev/null 2>&1; then
    gh_api --method PATCH "/repos/$OWNER/$REPO/labels/$encoded_label" --input "$label_body" >/dev/null
  else
    gh_api --method POST "/repos/$OWNER/$REPO/labels" --input "$label_body" >/dev/null
  fi

  rm -f "$label_body"
}

put_environment staging
sync_deployment_policies staging branch:main
put_environment production
sync_deployment_policies production 'tag:v*'

ensure_label type:bug d73a4a 'Reproducible defect; mirrors Beads type bug.'
ensure_label type:feature a2eeef 'New capability or behavior change; mirrors Beads type feature.'
ensure_label type:epic 5319e7 'Large body of related work; mirrors Beads type epic.'
ensure_label type:issue cfd3d7 'General human-reported work item; mirrors Beads type task.'
ensure_label needs-triage ffb347 'Needs review before conversion into Beads work.'

cat <<SUMMARY

Repository bootstrap summary
- Rulesets applied from: $RULESET_DIR
- Actions default GITHUB_TOKEN permissions set to read-only.
- GitHub Actions restricted to GitHub-owned, verified, and explicit allowlist patterns.
- SHA pinning requested where the repository API supports it.
- Dependabot alerts and automated security fixes enabled.
- Immutable GitHub Releases enabled.
- staging environment allows deployments from main.
- production environment allows deployments from v* tags only.
- Work item labels synchronized for bug, feature, epic, and issue intake.
- Merge settings set to delete branches on merge and squash-only.

Human-only follow-up
- Confirm required workflow job names exist: build-test, lint, secrets-scan, sast, deps-scan, iac-scan, actions-lint.
- Add environment secrets or variables such as AWS role ARNs after AWS OIDC roles exist.
- Enable Codex automatic review or configure CLAUDE_CODE_OAUTH_TOKEN for the committed AI-review workflow.
- Configure commit signing before applying signed-commit rulesets to active work.
- Org-wide Actions restrictions, template-repository flags, and org security defaults require owner/admin review.
- Private GitHub Team repositories do not have Enterprise environment required reviewers or wait timers; this template uses branch/tag deployment policies instead.
SUMMARY
