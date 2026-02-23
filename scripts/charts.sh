#!/usr/bin/env bash
# Generate SVG line charts from git history:
#   docs/charts/commits-per-week.svg
#   docs/charts/loc-per-week.svg
#
# Usage: ./scripts/charts.sh

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

OUT_DIR="docs/charts"
mkdir -p "$OUT_DIR"

WEEKS=52
NOW_EPOCH=$(date +%s)
START_EPOCH=$((NOW_EPOCH - WEEKS * 7 * 86400))

# ── Extract raw data ──────────────────────────────────────────────
git log --all --format="COMMIT %at" --numstat --since="${WEEKS} weeks ago" | awk \
    -v weeks="$WEEKS" -v start="$START_EPOCH" '
BEGIN {
    for (i = 0; i < weeks; i++) {
        commits[i] = 0; added[i] = 0; deleted[i] = 0
    }
    current_week = -1
}
/^COMMIT / {
    epoch = $2
    current_week = int((epoch - start) / (7 * 86400))
    if (current_week >= 0 && current_week < weeks) {
        commits[current_week]++
    }
    next
}
/^[0-9]/ {
    if (current_week >= 0 && current_week < weeks) {
        added[current_week] += $1
        deleted[current_week] += $2
    }
}
END {
    for (i = 0; i < weeks; i++) {
        print commits[i], added[i], deleted[i]
    }
}
' > /tmp/ag_chart_data.txt

# Cumulative LOC baseline (before the window)
BASELINE=$(git log --all --format="" --numstat --until="${WEEKS} weeks ago" 2>/dev/null | awk '
    /^[0-9]/ { a += $1; d += $2 } END { v = a - d; print (v > 0 ? v : 0) }
')
BASELINE=${BASELINE:-0}

# Build arrays
COMMIT_VALS=()
LOC_VALS=()
cumulative=$BASELINE

while IFS=' ' read -r c a d; do
    COMMIT_VALS+=("$c")
    cumulative=$((cumulative + a - d))
    if (( cumulative < 0 )); then cumulative=0; fi
    LOC_VALS+=("$cumulative")
done < /tmp/ag_chart_data.txt

# Month labels
MONTH_LABELS=()
for i in $(seq 0 $((WEEKS - 1))); do
    week_epoch=$((START_EPOCH + i * 7 * 86400))
    MONTH_LABELS+=("$(date -r "$week_epoch" +"%b" 2>/dev/null || date -d "@$week_epoch" +"%b" 2>/dev/null)")
done

# ── SVG generator ──────────────────────────────────────────────────
generate_svg() {
    local title="$1" output="$2" color="$3" fill_color="$4"
    shift 4
    local values=("$@")
    local n=${#values[@]}

    local W=800 H=280
    local PL=70 PR=20 PT=50 PB=55
    local CW=$((W - PL - PR))
    local CH=$((H - PT - PB))

    # Max value
    local max_val=0
    for v in "${values[@]}"; do
        (( v > max_val )) && max_val=$v
    done
    (( max_val == 0 )) && max_val=1

    # Round up for nice grid
    if (( max_val > 1000 )); then
        max_val=$(( ((max_val / 500) + 1) * 500 ))
    elif (( max_val > 100 )); then
        max_val=$(( ((max_val / 100) + 1) * 100 ))
    elif (( max_val > 10 )); then
        max_val=$(( ((max_val / 5) + 1) * 5 ))
    else
        max_val=$((max_val + 1))
    fi

    # Build points string
    local points="" fill_pts=""
    local last_x="" last_y=""
    for i in $(seq 0 $((n - 1))); do
        local v=${values[$i]}
        local x y
        x=$(awk "BEGIN { printf \"%.1f\", $PL + $i * $CW / ($n - 1.0) }")
        y=$(awk "BEGIN { printf \"%.1f\", $PT + $CH - ($v / $max_val * $CH) }")
        points="$points $x,$y"
        fill_pts="$fill_pts $x,$y"
        last_x=$x; last_y=$y
    done
    fill_pts="$PL,$((H - PB)) $fill_pts $last_x,$((H - PB))"

    # Grid lines + Y labels
    local grid=""
    for j in 0 1 2 3 4; do
        local val y
        val=$(awk "BEGIN { printf \"%d\", $max_val * $j / 4 }")
        y=$(awk "BEGIN { printf \"%.1f\", $PT + $CH - ($j / 4.0 * $CH) }")
        grid="$grid<line x1=\"$PL\" y1=\"$y\" x2=\"$((W - PR))\" y2=\"$y\" stroke=\"#21262d\"/>"
        grid="$grid<text x=\"$((PL - 10))\" y=\"$y\" text-anchor=\"end\" dominant-baseline=\"middle\" fill=\"#8b949e\" font-size=\"11\">$val</text>"
    done

    # X labels (every ~8 weeks)
    local xlabels=""
    for i in $(seq 0 8 $((n - 1))); do
        local x
        x=$(awk "BEGIN { printf \"%.1f\", $PL + $i * $CW / ($n - 1.0) }")
        xlabels="$xlabels<text x=\"$x\" y=\"$((H - PB + 22))\" text-anchor=\"middle\" fill=\"#8b949e\" font-size=\"11\">${MONTH_LABELS[$i]}</text>"
    done

    local last_val="${values[$((n - 1))]}"

    cat > "$output" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 $W $H" width="$W" height="$H">
  <style>text { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }</style>
  <rect width="$W" height="$H" rx="6" fill="#0d1117"/>
  <text x="$PL" y="30" fill="#e6edf3" font-size="16" font-weight="600">$title</text>
  <text x="$((W - PR))" y="30" fill="$color" font-size="14" font-weight="600" text-anchor="end">$last_val</text>
  $grid
  <polygon points="$fill_pts" fill="$fill_color"/>
  <polyline points="$points" fill="none" stroke="$color" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="$last_x" cy="$last_y" r="4" fill="$color"/>
  $xlabels
</svg>
EOF

    echo "  $output ($last_val current)"
}

# ── Generate ──────────────────────────────────────────────────────
echo "Generating charts from ${WEEKS} weeks of git history..."
generate_svg "Commits per week" "$OUT_DIR/commits-per-week.svg" "#39d353" "#39d35320" "${COMMIT_VALS[@]}"
generate_svg "Lines of code" "$OUT_DIR/loc-per-week.svg" "#58a6ff" "#58a6ff20" "${LOC_VALS[@]}"
rm -f /tmp/ag_chart_data.txt
echo "Done."
