#!/usr/bin/env bash
#
# Generate an interactive HTML timeline from the git history.
# Usage: ./scripts/timeline.sh [days] [output]
#   days   — how many days of history to include (default: 5)
#   output — path for the HTML file          (default: docs/timeline.html)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

DAYS="${1:-all}"
OUT="${2:-docs/timeline.html}"

# Collect commits as JSON array
COMMITS="["
first=true
if [ "$DAYS" = "all" ]; then
    LOG_ARGS="--format=%H|%aI|%s --reverse"
else
    LOG_ARGS="--since=${DAYS} days ago --format=%H|%aI|%s --reverse"
fi
while IFS='|' read -r hash date subject; do
    # Escape double-quotes and backslashes in subject
    subject="${subject//\\/\\\\}"
    subject="${subject//\"/\\\"}"
    if [ "$first" = true ]; then
        first=false
    else
        COMMITS+=","
    fi
    COMMITS+="{\"hash\":\"${hash:0:10}\",\"date\":\"$date\",\"subject\":\"$subject\"}"
done < <(git log $LOG_ARGS)
COMMITS+="]"

cat > "$OUT" << 'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit Development Timeline</title>
<style>
  :root {
    --bg: #0d1117; --surface: #161b22; --border: #30363d;
    --text: #e6edf3; --muted: #8b949e; --accent: #58a6ff;
    --green: #3fb950; --orange: #d29922; --purple: #bc8cff;
    --red: #f85149; --teal: #39d2c0;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }

  header { padding: 24px 32px 16px; border-bottom: 1px solid var(--border); }
  header h1 { font-size: 22px; font-weight: 600; }
  header p { color: var(--muted); margin-top: 4px; font-size: 14px; }

  .controls { display: flex; gap: 8px; flex-wrap: wrap; padding: 16px 32px; border-bottom: 1px solid var(--border); align-items: center; }
  .controls label { color: var(--muted); font-size: 13px; }
  .controls select, .controls input {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 6px; padding: 5px 10px; font-size: 13px;
  }
  .pill { display: inline-block; padding: 2px 10px; border-radius: 12px; font-size: 11px; font-weight: 600; cursor: pointer; border: 1px solid transparent; margin: 1px; }
  .pill.active { border-color: var(--accent); }

  .stats { display: flex; gap: 24px; padding: 16px 32px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .stat-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 12px 18px; min-width: 140px; }
  .stat-card .num { font-size: 28px; font-weight: 700; }
  .stat-card .label { color: var(--muted); font-size: 12px; margin-top: 2px; }

  .chart-container { padding: 16px 32px; }
  .chart-title { font-size: 14px; font-weight: 600; margin-bottom: 8px; }
  .chart-area { position: relative; }
  .chart-guidelines { position: absolute; top: 0; left: 0; right: 0; height: 120px; pointer-events: none; z-index: 0; }
  .chart-guideline { position: absolute; left: 0; right: 0; border-top: 1px dashed var(--border); }
  .chart-guideline-label { position: absolute; right: 4px; top: -12px; font-size: 9px; color: var(--muted); }
  .hour-chart { display: flex; align-items: flex-end; gap: 1px; height: 120px; padding: 0; position: relative; z-index: 1; }
  .hour-bar-wrap { flex: 1; display: flex; flex-direction: column; align-items: center; height: 100%; justify-content: flex-end; position: relative; }
  .hour-bar-stack { width: 100%; min-width: 2px; display: flex; flex-direction: column-reverse; cursor: pointer; position: relative; border-radius: 2px 2px 0 0; overflow: hidden; }
  .hour-bar-stack:hover { opacity: 0.8; }
  .hour-bar-segment { width: 100%; min-height: 0; }
  .hour-label { font-size: 9px; color: var(--muted); margin-top: 3px; writing-mode: vertical-rl; text-orientation: mixed; max-height: 48px; overflow: hidden; }
  .day-sep { border-left: 1px dashed var(--border); height: 100%; position: absolute; left: -1px; top: 0; pointer-events: none; }
  .x-axis { display: flex; gap: 1px; border-top: 1px solid var(--border); margin-top: 2px; }
  .x-axis-tick { flex: 1; text-align: center; font-size: 8px; color: var(--muted); padding-top: 3px; overflow: hidden; white-space: nowrap; }

  .timeline { padding: 0 32px 48px; }
  .day-group { margin-top: 24px; }
  .day-header { font-size: 14px; font-weight: 600; color: var(--accent); margin-bottom: 8px; position: sticky; top: 0; background: var(--bg); padding: 6px 0; z-index: 2; border-bottom: 1px solid var(--border); }
  .hour-group { margin-left: 12px; margin-bottom: 12px; }
  .hour-header { font-size: 12px; color: var(--muted); margin-bottom: 4px; display: flex; align-items: center; gap: 8px; }
  .hour-header .count-badge { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 0 7px; font-size: 11px; }

  .commit-list { list-style: none; margin-left: 20px; border-left: 2px solid var(--border); padding-left: 16px; }
  .commit-item { padding: 4px 0; font-size: 13px; display: flex; gap: 8px; align-items: baseline; position: relative; }
  .commit-item::before { content: ""; position: absolute; left: -21px; top: 10px; width: 8px; height: 8px; border-radius: 50%; background: var(--border); }
  .commit-item.milestone::before { background: var(--green); box-shadow: 0 0 6px var(--green); }
  .commit-item .hash { font-family: "SFMono-Regular", Consolas, monospace; color: var(--muted); font-size: 12px; flex-shrink: 0; }
  .commit-item .time { color: var(--muted); font-size: 11px; flex-shrink: 0; width: 44px; }
  .commit-item .msg { flex: 1; }
  .commit-item .tag { font-size: 10px; font-weight: 600; padding: 1px 7px; border-radius: 10px; flex-shrink: 0; }

  .tag-feat { background: rgba(63,185,80,0.15); color: var(--green); }
  .tag-fix { background: rgba(248,81,73,0.15); color: var(--red); }
  .tag-test { background: rgba(188,140,255,0.15); color: var(--purple); }
  .tag-merge { background: rgba(88,166,255,0.15); color: var(--accent); }
  .tag-docs { background: rgba(210,153,34,0.15); color: var(--orange); }
  .tag-refactor { background: rgba(57,210,192,0.15); color: var(--teal); }
  .tag-other { background: rgba(139,148,158,0.15); color: var(--muted); }

  .hidden { display: none !important; }

  .tooltip { position: fixed; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 10px 14px; font-size: 12px; pointer-events: none; z-index: 100; max-width: 340px; box-shadow: 0 4px 12px rgba(0,0,0,0.4); }
  .tooltip .tt-date { color: var(--accent); font-weight: 600; }
  .tooltip .tt-count { color: var(--muted); margin-top: 2px; }
  .tooltip .tt-items { margin-top: 6px; max-height: 160px; overflow-y: auto; }
  .tooltip .tt-item { padding: 2px 0; border-bottom: 1px solid var(--border); }
  .tooltip .tt-item:last-child { border: none; }
</style>
</head>
<body>

<header>
  <h1>Grit Development Timeline</h1>
  <p>Commit activity over the last <span id="daySpan"></span> days &mdash; <span id="totalCommits"></span> commits</p>
</header>

<div class="controls">
  <label>Filter:</label>
  <span class="pill active" data-filter="all" style="background:rgba(230,237,243,0.1);color:var(--text)">All</span>
  <span class="pill" data-filter="feat" style="background:rgba(63,185,80,0.15);color:var(--green)">Features</span>
  <span class="pill" data-filter="fix" style="background:rgba(248,81,73,0.15);color:var(--red)">Fixes</span>
  <span class="pill" data-filter="test" style="background:rgba(188,140,255,0.15);color:var(--purple)">Tests</span>
  <span class="pill" data-filter="merge" style="background:rgba(88,166,255,0.15);color:var(--accent)">Merges</span>
  <span class="pill" data-filter="docs" style="background:rgba(210,153,34,0.15);color:var(--orange)">Docs</span>
  <label style="margin-left:auto;">Search:</label>
  <input type="text" id="search" placeholder="e.g. blame, clone, merge...">
</div>

<div class="stats" id="stats"></div>

<div class="chart-container">
  <div class="chart-title">Commits per hour</div>
  <div class="chart-area">
    <div class="chart-guidelines" id="chartGuidelines"></div>
    <div class="hour-chart" id="hourChart"></div>
  </div>
  <div class="x-axis" id="xAxis"></div>
</div>

<div class="timeline" id="timeline"></div>
<div class="tooltip hidden" id="tooltip"></div>

<script>
const COMMITS_DATA = COMMIT_JSON_PLACEHOLDER;

// ── Categorization ──────────────────────────────────────────────
function categorize(subject) {
  const s = subject.toLowerCase();
  if (/^merge /.test(s)) return 'merge';
  if (/^docs:/.test(s) || /^doc:/.test(s) || /auto-regen dashboard/.test(s)) return 'docs';
  if (/^tests?[\s:(]/.test(s) || /^tests?:/.test(s)) return 'test';
  if (/^fix[\s:(]/.test(s) || /^fix:/.test(s)) return 'fix';
  if (/^refactor/.test(s) || /^perf/.test(s) || /^chore/.test(s) || /^cargo /.test(s) || /^license/.test(s)) return 'refactor';
  // Anything that adds/implements a command or feature
  if (/^feat/.test(s) || /^add[: ]/.test(s) || /: add /.test(s) || /: implement/.test(s) || /^implement/.test(s)) return 'feat';
  // Commands being implemented (e.g. "blame: add -C flag")
  if (/^[a-z-]+: /.test(s) && (/add|implement|support|handle|enable/.test(s))) return 'feat';
  if (/^[a-z-]+: /.test(s) && (/fix|correct|repair|prevent/.test(s))) return 'fix';
  if (/^[a-z-]+: /.test(s)) return 'feat';
  return 'other';
}

function isMilestone(subject) {
  const s = subject.toLowerCase();
  // New command implementations, major features
  if (/^(feat|add)[:(]/.test(s)) return true;
  if (/: implement /.test(s)) return true;
  if (/: add .*(flag|support|mode|command)/.test(s)) return true;
  if (/implement .*(command|support)/.test(s)) return true;
  // Significant new capabilities indicated by subject patterns
  if (/^[a-z-]+: add /.test(s) && !(/auto-regen/.test(s))) return true;
  if (/^clone:|^fetch:|^push:|^merge:|^rebase:|^blame:|^grep:|^stash:|^cherry-pick:|^am:/.test(s)) return true;
  return false;
}

const tagLabel = { feat: 'feature', fix: 'fix', test: 'test', merge: 'merge', docs: 'docs', refactor: 'chore', other: '' };

// ── Parse & enrich ──────────────────────────────────────────────
const commits = COMMITS_DATA.map(c => {
  const d = new Date(c.date);
  return { ...c, ts: d, cat: categorize(c.subject), milestone: isMilestone(c.subject) };
});

const earliest = commits[0]?.ts ?? new Date();
const latest = commits[commits.length - 1]?.ts ?? new Date();
const daySpan = Math.ceil((latest - earliest) / 86400000) || 1;
document.getElementById('daySpan').textContent = daySpan;
document.getElementById('totalCommits').textContent = commits.length;

// ── Stats ───────────────────────────────────────────────────────
function renderStats(filtered) {
  const cats = {};
  filtered.forEach(c => { cats[c.cat] = (cats[c.cat] || 0) + 1; });
  const milestones = filtered.filter(c => c.milestone).length;
  const html = `
    <div class="stat-card"><div class="num">${filtered.length}</div><div class="label">Total commits</div></div>
    <div class="stat-card"><div class="num" style="color:var(--green)">${cats.feat || 0}</div><div class="label">Features</div></div>
    <div class="stat-card"><div class="num" style="color:var(--red)">${cats.fix || 0}</div><div class="label">Fixes</div></div>
    <div class="stat-card"><div class="num" style="color:var(--purple)">${cats.test || 0}</div><div class="label">Tests</div></div>
    <div class="stat-card"><div class="num" style="color:var(--green)">${milestones}</div><div class="label">Milestones</div></div>
  `;
  document.getElementById('stats').innerHTML = html;
}

// ── Hour chart ──────────────────────────────────────────────────
const catColors = { feat: 'var(--green)', fix: 'var(--red)', test: 'var(--purple)', merge: 'var(--accent)', docs: 'var(--orange)', refactor: 'var(--teal)', other: 'var(--muted)' };
const catOrder = ['feat', 'fix', 'test', 'merge', 'docs', 'refactor', 'other'];

function hourKey(d) {
  return d.toISOString().slice(0, 13); // "2026-04-02T14"
}
function renderHourChart(filtered) {
  // Build hourly buckets across full range
  const buckets = new Map();
  const start = new Date(earliest); start.setMinutes(0,0,0);
  const end = new Date(latest); end.setHours(end.getHours()+1, 0, 0, 0);
  for (let t = new Date(start); t <= end; t.setHours(t.getHours()+1)) {
    buckets.set(hourKey(t), { date: new Date(t), commits: [] });
  }
  filtered.forEach(c => {
    const k = hourKey(c.ts);
    if (buckets.has(k)) buckets.get(k).commits.push(c);
  });

  const entries = [...buckets.values()];
  const maxCount = Math.max(1, ...entries.map(e => e.commits.length));

  // ── Guidelines ──
  const guideEl = document.getElementById('chartGuidelines');
  guideEl.innerHTML = '';
  // Pick nice guideline intervals
  const chartH = 120;
  const niceSteps = [1, 2, 5, 10, 20, 50, 100];
  const step = niceSteps.find(s => maxCount / s <= 5) || Math.ceil(maxCount / 5);
  for (let v = step; v <= maxCount; v += step) {
    const pct = (v / maxCount) * 100;
    const line = document.createElement('div');
    line.className = 'chart-guideline';
    line.style.bottom = pct + '%';
    line.style.top = 'auto';
    // position from bottom
    line.style.position = 'absolute';
    line.style.bottom = pct + '%';
    line.style.removeProperty('top');
    const lbl = document.createElement('span');
    lbl.className = 'chart-guideline-label';
    lbl.textContent = v;
    lbl.style.top = 'auto';
    lbl.style.bottom = '-1px';
    line.appendChild(lbl);
    guideEl.appendChild(line);
  }

  // ── Bars ──
  const chart = document.getElementById('hourChart');
  chart.innerHTML = '';
  const xAxis = document.getElementById('xAxis');
  xAxis.innerHTML = '';

  let prevDay = '';
  entries.forEach(e => {
    const wrap = document.createElement('div');
    wrap.className = 'hour-bar-wrap';
    const count = e.commits.length;

    const dayStr = e.date.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric', timeZone: 'UTC' });
    const hourStr = e.date.toLocaleTimeString('en-US', { hour: '2-digit', hour12: false, timeZone: 'UTC' });

    if (dayStr !== prevDay) {
      const sep = document.createElement('div');
      sep.className = 'day-sep';
      wrap.appendChild(sep);
      prevDay = dayStr;
    }

    if (count > 0) {
      const stack = document.createElement('div');
      stack.className = 'hour-bar-stack';
      const totalPct = (count / maxCount) * 100;
      stack.style.height = totalPct + '%';
      stack.style.minHeight = '3px';

      // Count commits per category and build stacked segments
      const catCounts = {};
      e.commits.forEach(c => { catCounts[c.cat] = (catCounts[c.cat]||0)+1; });
      catOrder.forEach(cat => {
        if (!catCounts[cat]) return;
        const seg = document.createElement('div');
        seg.className = 'hour-bar-segment';
        seg.style.height = ((catCounts[cat] / count) * 100) + '%';
        seg.style.background = catColors[cat] || 'var(--muted)';
        stack.appendChild(seg);
      });

      stack.addEventListener('mouseenter', (ev) => {
        const tt = document.getElementById('tooltip');
        tt.classList.remove('hidden');
        let items = e.commits.slice(0, 8).map(c =>
          `<div class="tt-item">${c.hash.slice(0,7)} ${c.subject.slice(0,60)}</div>`
        ).join('');
        if (e.commits.length > 8) items += `<div class="tt-item" style="color:var(--muted)">+${e.commits.length - 8} more</div>`;
        tt.innerHTML = `<div class="tt-date">${dayStr} ${hourStr}:00 UTC</div><div class="tt-count">${count} commits</div><div class="tt-items">${items}</div>`;
        const r = ev.target.getBoundingClientRect();
        tt.style.left = Math.min(r.left, window.innerWidth - 360) + 'px';
        tt.style.top = (r.top - tt.offsetHeight - 8) + 'px';
      });
      stack.addEventListener('mouseleave', () => { document.getElementById('tooltip').classList.add('hidden'); });
      wrap.appendChild(stack);
    }

    chart.appendChild(wrap);

    // ── X-axis tick ──
    const tick = document.createElement('div');
    tick.className = 'x-axis-tick';
    const h = parseInt(hourStr);
    if (h % 6 === 0) {
      tick.textContent = hourStr + ':00';
      if (h === 0) tick.textContent = dayStr.split(',')[0];
    }
    xAxis.appendChild(tick);
  });
}

// ── Commit timeline ─────────────────────────────────────────────
function renderTimeline(filtered) {
  // Group by day then hour
  const days = new Map();
  filtered.forEach(c => {
    const dayKey = c.ts.toLocaleDateString('en-US', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric', timeZone: 'UTC' });
    if (!days.has(dayKey)) days.set(dayKey, new Map());
    const hourKey = c.ts.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false, timeZone: 'UTC' }).slice(0,2) + ':00';
    const hm = days.get(dayKey);
    if (!hm.has(hourKey)) hm.set(hourKey, []);
    hm.get(hourKey).push(c);
  });

  const el = document.getElementById('timeline');
  el.innerHTML = '';

  // Reverse: newest day first
  const dayEntries = [...days.entries()].reverse();
  for (const [dayKey, hours] of dayEntries) {
    const dg = document.createElement('div');
    dg.className = 'day-group';
    const dh = document.createElement('div');
    dh.className = 'day-header';
    const dayTotal = [...hours.values()].reduce((s, h) => s + h.length, 0);
    dh.textContent = `${dayKey} (${dayTotal} commits)`;
    dg.appendChild(dh);

    const hourEntries = [...hours.entries()].reverse();
    for (const [hourKey, hCommits] of hourEntries) {
      const hg = document.createElement('div');
      hg.className = 'hour-group';
      const hh = document.createElement('div');
      hh.className = 'hour-header';
      hh.innerHTML = `${hourKey} UTC <span class="count-badge">${hCommits.length}</span>`;
      hg.appendChild(hh);

      const ul = document.createElement('ul');
      ul.className = 'commit-list';
      // Show newest first within hour
      for (const c of [...hCommits].reverse()) {
        const li = document.createElement('li');
        li.className = 'commit-item' + (c.milestone ? ' milestone' : '');
        const time = c.ts.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false, timeZone: 'UTC' });
        const tagCls = 'tag tag-' + c.cat;
        const tagText = tagLabel[c.cat] || '';
        li.innerHTML = `<span class="time">${time}</span><span class="hash">${c.hash}</span><span class="msg">${escHtml(c.subject)}</span>${tagText ? `<span class="${tagCls}">${tagText}</span>` : ''}`;
        ul.appendChild(li);
      }
      hg.appendChild(ul);
      dg.appendChild(hg);
    }
    el.appendChild(dg);
  }
}

function escHtml(s) { const d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

// ── Filtering ───────────────────────────────────────────────────
let activeFilter = 'all';
let searchTerm = '';

function applyFilters() {
  let filtered = commits;
  if (activeFilter !== 'all') filtered = filtered.filter(c => c.cat === activeFilter);
  if (searchTerm) {
    const q = searchTerm.toLowerCase();
    filtered = filtered.filter(c => c.subject.toLowerCase().includes(q));
  }
  renderStats(filtered);
  renderHourChart(filtered);
  renderTimeline(filtered);
}

document.querySelectorAll('.pill').forEach(p => {
  p.addEventListener('click', () => {
    document.querySelectorAll('.pill').forEach(x => x.classList.remove('active'));
    p.classList.add('active');
    activeFilter = p.dataset.filter;
    applyFilters();
  });
});
document.getElementById('search').addEventListener('input', e => {
  searchTerm = e.target.value;
  applyFilters();
});

applyFilters();
</script>
</body>
</html>
HTMLEOF

# Inject the real commit JSON
# Use a temp file to avoid issues with large data in sed
python3 -c "
import sys, json
html = open('$OUT').read()
commits = open('/dev/stdin').read().strip()
html = html.replace('COMMIT_JSON_PLACEHOLDER', commits, 1)
open('$OUT', 'w').write(html)
" << PYEOF
$COMMITS
PYEOF

echo "Generated $OUT with $(echo "$COMMITS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo '?') commits"
