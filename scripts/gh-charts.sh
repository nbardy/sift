#!/usr/bin/env bash
# Generate a WebP image with 2 line charts across ALL GitHub repos:
#   - Commits per week (from GitHub contributions calendar)
#   - Lines of code changed per week (additions - deletions, aggregated)
#
# Output: gh-activity.webp
# Requires: gh (authenticated), python3 (with Pillow), cwebp
#
# Usage: ./scripts/gh-charts.sh [username] [output.webp]

set -euo pipefail

USERNAME="${1:-nbardy}"
OUTPUT="${2:-gh-activity.webp}"

echo "Fetching GitHub activity for $USERNAME..."

python3 - "$USERNAME" "$OUTPUT" << 'PYEOF'
import json
import subprocess
import sys
import time
from datetime import datetime, timedelta

USERNAME = sys.argv[1]
OUTPUT = sys.argv[2]

def gh(args):
    r = subprocess.run(["gh"] + args, capture_output=True, text=True)
    if r.returncode != 0:
        return None
    try:
        return json.loads(r.stdout)
    except json.JSONDecodeError:
        return None

def gh_gql(query):
    return gh(["api", "graphql", "-f", f"query={query}"])

# ── 1. Commits per week ──────────────────────────────────────────
print("  Fetching contributions...", file=sys.stderr, flush=True)

data = gh_gql(f'''{{
  user(login: "{USERNAME}") {{
    contributionsCollection {{
      contributionCalendar {{
        totalContributions
        weeks {{ contributionDays {{ contributionCount date }} }}
      }}
    }}
  }}
}}''')

cal = data["data"]["user"]["contributionsCollection"]["contributionCalendar"]
total = cal["totalContributions"]

commits_per_week = []
week_labels = []
for w in cal["weeks"]:
    days = w["contributionDays"]
    commits_per_week.append(sum(d["contributionCount"] for d in days))
    week_labels.append(days[0]["date"])

n_weeks = len(commits_per_week)
print(f"  {n_weeks} weeks, {total} total contributions", file=sys.stderr, flush=True)

# ── 2. LOC per week ──────────────────────────────────────────────
print("  Fetching repos...", file=sys.stderr, flush=True)

repos_data = gh_gql(f'''{{
  user(login: "{USERNAME}") {{
    repositories(first: 100, orderBy: {{field: PUSHED_AT, direction: DESC}}, ownerAffiliations: OWNER) {{
      nodes {{ nameWithOwner pushedAt isFork }}
    }}
  }}
}}''')

cutoff = (datetime.now() - timedelta(days=365)).isoformat()
repos = [
    r["nameWithOwner"]
    for r in repos_data["data"]["user"]["repositories"]["nodes"]
    if r["pushedAt"] and r["pushedAt"] > cutoff and not r["isFork"]
]

print(f"  {len(repos)} active repos, warming stats API...", file=sys.stderr, flush=True)

# Phase 1: Warm all repos (trigger 202 computation)
for repo in repos:
    gh(["api", f"repos/{repo}/stats/code_frequency"])

print("  Waiting 5s for GitHub to compute stats...", file=sys.stderr, flush=True)
time.sleep(5)

# Phase 2: Fetch with retry
week_epoch_to_idx = {}
for i, label in enumerate(week_labels):
    dt = datetime.strptime(label, "%Y-%m-%d")
    epoch = int(dt.timestamp())
    week_epoch_to_idx[epoch] = i

loc_added = [0] * n_weeks
loc_deleted = [0] * n_weeks
fetched = 0
failed = 0

for repo in repos:
    freq = gh(["api", f"repos/{repo}/stats/code_frequency"])
    if not isinstance(freq, list) or len(freq) == 0:
        time.sleep(2)
        freq = gh(["api", f"repos/{repo}/stats/code_frequency"])
    if isinstance(freq, list) and len(freq) > 0:
        for entry in freq:
            ts = entry[0]
            if ts in week_epoch_to_idx:
                idx = week_epoch_to_idx[ts]
                loc_added[idx] += entry[1]
                loc_deleted[idx] += abs(entry[2])
        fetched += 1
    else:
        failed += 1
    print(f"\r  {fetched}/{len(repos)} repos fetched ({failed} pending)   ", end="", file=sys.stderr, flush=True)

print(file=sys.stderr, flush=True)

# Cumulative LOC
loc_cumulative = []
running = 0
for a, d in zip(loc_added, loc_deleted):
    running += a - d
    loc_cumulative.append(max(0, running))

# If LOC is all zeros, use added-per-week as fallback display
loc_display = loc_cumulative
loc_title = "Lines of code"
if max(loc_cumulative) == 0 and max(loc_added) > 0:
    loc_display = loc_added
    loc_title = "Lines added per week"
elif max(loc_cumulative) == 0:
    # All data still computing — show a flat line with a note
    loc_display = [0] * n_weeks
    loc_title = "Lines of code (stats computing, re-run in 1min)"

print(f"  {fetched} repos with data, {failed} still computing", file=sys.stderr, flush=True)

# ── 3. Generate image ────────────────────────────────────────────
from PIL import Image, ImageDraw, ImageFont

W, H = 900, 500
BG = (13, 17, 23)
GRID = (33, 38, 45)
TEXT = (230, 237, 243)
DIM = (139, 148, 158)
GREEN = (57, 211, 83)
BLUE = (88, 166, 255)
GREEN_FILL = (57, 211, 83, 35)
BLUE_FILL = (88, 166, 255, 35)

CHART_H = 150
PAD_L, PAD_R = 75, 30
GAP = 60
CHART_W = W - PAD_L - PAD_R

img = Image.new("RGBA", (W, H), BG)
draw = ImageDraw.Draw(img)

# Font
for path in [
    "/System/Library/Fonts/SFCompact.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]:
    try:
        ftitle = ImageFont.truetype(path, 15)
        fsmall = ImageFont.truetype(path, 11)
        fval = ImageFont.truetype(path, 13)
        break
    except:
        continue
else:
    ftitle = fsmall = fval = ImageFont.load_default()

def draw_chart(values, top_y, title, color, fill_color):
    n = len(values)
    mx = max(values) if max(values) > 0 else 1
    if mx > 5000:   mx = ((mx // 1000) + 1) * 1000
    elif mx > 1000: mx = ((mx // 500) + 1) * 500
    elif mx > 100:  mx = ((mx // 50) + 1) * 50
    elif mx > 10:   mx = ((mx // 5) + 1) * 5
    else:           mx += 1

    # Title + current value
    draw.text((PAD_L, top_y - 25), title, fill=TEXT, font=ftitle)
    cv = f"{values[-1]:,}"
    bb = draw.textbbox((0, 0), cv, font=fval)
    draw.text((W - PAD_R - (bb[2] - bb[0]), top_y - 23), cv, fill=color, font=fval)

    # Grid
    for j in range(5):
        y = top_y + CHART_H - (j / 4.0 * CHART_H)
        draw.line([(PAD_L, y), (W - PAD_R, y)], fill=GRID, width=1)
        v = f"{int(mx * j / 4):,}"
        bb = draw.textbbox((0, 0), v, font=fsmall)
        draw.text((PAD_L - 8 - (bb[2] - bb[0]), y - 6), v, fill=DIM, font=fsmall)

    # Points
    pts = []
    for i in range(n):
        x = PAD_L + i * CHART_W / max(n - 1, 1)
        y = top_y + CHART_H - (values[i] / mx * CHART_H)
        pts.append((x, y))

    # Fill
    fp = [(PAD_L, top_y + CHART_H)] + pts + [(pts[-1][0], top_y + CHART_H)]
    draw.polygon(fp, fill=fill_color)

    # Line
    if len(pts) > 1:
        draw.line(pts, fill=color, width=2)

    # Dot
    draw.ellipse([pts[-1][0]-4, pts[-1][1]-4, pts[-1][0]+4, pts[-1][1]+4], fill=color)

    # Month labels
    for i in range(0, n, 8):
        x = PAD_L + i * CHART_W / max(n - 1, 1)
        dt = datetime.strptime(week_labels[i], "%Y-%m-%d")
        m = dt.strftime("%b")
        bb = draw.textbbox((0, 0), m, font=fsmall)
        draw.text((x - (bb[2] - bb[0]) / 2, top_y + CHART_H + 8), m, fill=DIM, font=fsmall)

# Header
hdr = f"@{USERNAME}  \u2022  {total:,} contributions  \u2022  {len(repos)} repos"
draw.text((PAD_L, 12), hdr, fill=DIM, font=fval)

c1 = 50
c2 = c1 + CHART_H + GAP
draw_chart(commits_per_week, c1, "Commits per week", GREEN, GREEN_FILL)
draw_chart(loc_display, c2, loc_title, BLUE, BLUE_FILL)

# Save as PNG, convert to WebP
img.save("/tmp/gh_chart.png", "PNG")
print(f"  Image: {W}x{H}", file=sys.stderr, flush=True)

import subprocess as sp
sp.run(["cwebp", "-q", "90", "/tmp/gh_chart.png", "-o", OUTPUT],
       capture_output=True)

import os
os.remove("/tmp/gh_chart.png")
print(f"Done: {OUTPUT} ({os.path.getsize(OUTPUT)//1024}KB)", file=sys.stderr, flush=True)
PYEOF
