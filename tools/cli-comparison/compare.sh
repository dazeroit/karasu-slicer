#!/bin/bash
# Usage: compare.sh <out-dir-a> <out-dir-b>: byte-compare every file of two runs (outputs,
# summaries, stderr, exit codes). Exits nonzero when anything differs.
A=$1; B=$2; bad=0
diff <(cd "$A" && find . -type f | sort) <(cd "$B" && find . -type f | sort) || bad=1
while read -r f; do
  [ -f "$B/$f" ] && ! cmp -s "$A/$f" "$B/$f" && { echo "DIFF $f"; bad=1; }
done < <(cd "$A" && find . -type f | sort)
if [ $bad = 0 ]; then echo "IDENTICAL $(cd "$A" && find . -type f | wc -l) files"; else echo "DIFFERENT"; exit 1; fi
