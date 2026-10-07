#!/usr/bin/env bash
# 自检：bash .github/scripts/next-version-test.sh
set -u
script="$(dirname "$0")/next-version.sh"
fail=0
check() { # 期望值 tag...
  local want="$1"; shift
  local got
  got="$(printf '%s\n' "$@" | bash "$script" 2>/dev/null)"
  if [ "$got" = "$want" ]; then echo "ok   [$*] -> $got"; else echo "FAIL [$*] -> '$got' (want $want)"; fail=1; fi
}
check 0.1.0 ""
check 0.1.0 v0.0.1
check 0.1.1 v0.1.0
check 0.1.9 v0.1.8
check 0.2.0 v0.1.9
check 1.0.0 v0.9.9
check 1.2.4 v1.2.3
check 0.2.0 v0.0.1 v0.1.0 v0.1.1 v0.1.2 v0.1.3 v0.1.4 v0.1.5 v0.1.6 v0.1.7 v0.1.8 v0.1.9 v0.1.10
check 0.2.0 v0.1.10 v0.1.9 v0.1.2
check 0.1.1 v0.1.0 nonsense v0.1.0-rc1
exit $fail
