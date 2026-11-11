#!/usr/bin/env bash
# Model-checks the bin (Bin.tla) with TLC: each promise on its own, the code as it is, and a
# failing one again under the assumption that nothing is put in the bin while it is emptied.
# Needs java. The checker (tla2tools 1.8.0) is fetched once into .worktrees/tlaplus/, which git
# ignores. Usage: .claude/skills/bin/check.sh [--outside] [--variant code|restore_renames|put_moves_first|unlocked|sweep_all|no_recheck|atomic|link_open|back_by_name|rename_open] [--files N] [--no-link]
# --outside: one Coxswain and one other program that takes no lock of ours (minutes, not seconds).
# --files N: N files instead of 2; a restore that links another program's file and a file the user
# saves at the place since needs 3 (#923).
# --no-link: a disk without hard links (Termux, FAT): move_new's empty file and rename (#931).
# Exit 0 when every promise holds, 1 when one fails (the trace is in the log it names), 2 on a
# tool error.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$(git -C "$here" rev-parse --git-common-dir)/.." && pwd)  # the main checkout, shared by worktrees
variant=code
outside=FALSE
nolink=FALSE
na=2
nc=2
while [ $# -gt 0 ]; do
    case $1 in
        --variant) variant=$2; shift 2 ;;
        --outside) outside=TRUE; na=1; shift ;;
        --files) nc=$2; shift 2 ;;
        --no-link) nolink=TRUE; shift ;;
        *) echo "usage: $0 [--outside] [--variant code|restore_renames|put_moves_first|unlocked|sweep_all|no_recheck|atomic|link_open|back_by_name|rename_open] [--files N] [--no-link]" >&2; exit 2 ;;
    esac
done

jar=${COXSWAIN_TLA_JAR:-$top/.worktrees/tlaplus/tla2tools-1.8.0.jar}
sum=011267fca25d0752cf18894dd585cd28b96960c112ea8a20b823ea770e7e10cd
if [ ! -f "$jar" ]; then
    mkdir -p "$(dirname "$jar")"
    echo "fetching tla2tools 1.8.0 to $jar"
    curl -sSfL -o "$jar.part" https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
    mv "$jar.part" "$jar"
fi
echo "$sum  $jar" | sha256sum -c --quiet - || { echo "$jar: not the pinned tla2tools 1.8.0" >&2; exit 2; }
command -v java >/dev/null || { echo "java is needed (any JRE 11 or newer)" >&2; exit 2; }

suffix=; [ $outside = FALSE ] || suffix=-outside
[ $nolink = FALSE ] || suffix=$suffix-nolink
out=$top/.worktrees/tlaplus/runs/$(date +%Y%m%d-%H%M%S)-$variant$suffix
mkdir -p "$out"
cp "$here/Bin.tla" "$out/"

# One TLC run: $1 the promise, $2 INVARIANT or PROPERTY, $3 PutDuringEmpty; prints PASS/FAIL/ERROR.
run() {
    local name=$1 kind=$2 during=$3 cfg="$out/$1-$3.cfg" log="$out/$1-$3.log"
    cat >"$cfg" <<EOF
CONSTANTS NA = $na NC = $nc NN = 2 NS = 2 Variant = "$variant" PutDuringEmpty = $during Outside = $outside NoLink = $nolink
INIT Init
NEXT Next
INVARIANT TypeOK
$kind $name
EOF
    if java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -config "$cfg" -workers auto -deadlock \
        -nowarning -metadir "$out/states" "$out/Bin.tla" >"$log" 2>&1; then
        echo PASS
    elif grep -q "is violated\|properties were violated" "$log"; then
        echo FAIL
    else
        echo ERROR
    fi
}

status=0
printf '%-16s %-6s %s\n' promise result "put during empty allowed / assumed not"
for p in NoLoss:INVARIANT Restorable:INVARIANT NeverOverwrite:PROPERTY TakesOnlyItsItem:PROPERTY; do
    name=${p%%:*} kind=${p##*:}
    r=$(run "$name" "$kind" TRUE)
    case $r in
        PASS) printf '%-16s %-6s\n' "$name" PASS ;;
        FAIL)
            status=1
            r2=$(run "$name" "$kind" FALSE)
            printf '%-16s %-6s with a put during empty: FAIL (%s); without: %s (%s)\n' \
                "$name" FAIL "$out/$name-TRUE.log" "$r2" "$out/$name-FALSE.log" ;;
        *) printf '%-16s %-6s see %s\n' "$name" ERROR "$out/$name-TRUE.log"; status=2 ;;
    esac
done
grep -h "states generated" "$out"/*.log | grep -v Progress | sort -t' ' -k1 -n | tail -1
rm -rf "$out/states"
exit $status
