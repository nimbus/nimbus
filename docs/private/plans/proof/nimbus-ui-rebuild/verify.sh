#!/usr/bin/env bash
# Verifier for docs/private/plans/nimbus-ui-rebuild-plan.html.
# Red on the baseline (main @ 67a7f3ffa); green when the rebuild lands.
# Usage: bash docs/private/plans/proof/nimbus-ui-rebuild/verify.sh
set -u
root="$(cd "$(dirname "$0")/../../../../.." && pwd)"
ui="$root/packages/nimbus-ui"
fail=0
check() { # check <label> <command...>
  local label="$1"; shift
  if "$@" >/dev/null 2>&1; then echo "ok   $label"; else echo "FAIL $label"; fail=$((fail + 1)); fi
}
has_dep() { grep -Eq "\"$1\": *\"" "$ui/package.json"; }
not_dep() { ! has_dep "$1"; }
absent() { [ ! -e "$ui/$1" ]; }
present() { [ -e "$ui/$1" ]; }
no_match() { ! grep -rqE "$1" "$ui/src" --include='*.ts' --include='*.tsx' --include='*.css'; }
only_importer() { # only_importer <package> <file>
  local hits; hits="$(grep -rlE "from [\"']$1" "$ui/src" --include='*.ts' --include='*.tsx' | sort)"
  [ "$hits" = "$ui/src/$2" ]
}
spec_for() { ls "$ui/src/routes/$1"*.spec.tsx >/dev/null 2>&1 || ls "$ui/src/routes/$1"/*.spec.tsx >/dev/null 2>&1; }

check "components.json exists (UIR1)"                       present components.json
check "@tanstack/react-table dependency (UIR1)"             has_dep "@tanstack/react-table"
check "@tanstack/react-virtual dependency (UIR1)"           has_dep "@tanstack/react-virtual"
check "@tanstack/charts dependency (UIR1)"                  has_dep "@tanstack/charts"
check "react-resizable-panels dependency (UIR1)"            has_dep "react-resizable-panels"
check "@fontsource/jetbrains-mono removed (UIR2)"           not_dep "@fontsource/jetbrains-mono"
check "Geist fonts self-hosted (UIR2)"                      present public/fonts/Geist-Regular.woff2
check "nimbus-ui:palette key removed (UIR2)"                no_match "nimbus-ui:palette"
check "no uppercase tracking outside sidebar (UIR2)"        bash -c "! grep -rlE 'uppercase' '$ui/src' --include='*.tsx' | grep -v '/shell/sidebar/'"
check "chart seam is the only @tanstack/charts importer (UIR3)" only_importer "@tanstack/charts" components/ui/chart.tsx
check "mascot component exists (UIR4)"                      present src/components/mascot.tsx
check "logo-mark.tsx removed (UIR4)"                        absent src/shell/logo-mark.tsx
check "sidebar exists (UIR5)"                               present src/shell/sidebar/sidebar.tsx
check "sub-panel exists (UIR6)"                             present src/shell/sub-panel.tsx
check "top-nav.tsx removed (UIR8)"                          absent src/shell/top-nav.tsx
check "status-bar.tsx removed (UIR8)"                       absent src/shell/status-bar.tsx
check "primary-drawer.tsx removed (UIR8)"                   absent src/shell/primary-drawer.tsx
check "sub-drawer.tsx removed (UIR8)"                       absent src/shell/sub-drawer.tsx
check "primary-drawer-collapsed key removed (UIR8)"         no_match "nimbus-ui:primary-drawer-collapsed"
check "no Coming soon nav items (UIR8)"                     no_match "Coming soon"
for r in developer/index developer/compute developer/storage developer/observability developer/settings operator/tenants operator/settings; do
  check "spec for routes/$r" spec_for "$r"
done
echo "verify: $fail failing"
[ "$fail" -eq 0 ]
