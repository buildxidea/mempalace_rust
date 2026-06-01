// MemPalace Live Graph viewer — minimal stub.
//
// Full live-graph SPA follows the design in REMAINING.md G5 follow-up.
// This stub keeps the index.html shell functional: it fetches
// /api/graph/stats and /api/graph/data?center=root on load, renders
// the center SVG with whatever nodes/edges come back (or an empty
// state), and connects to /api/graph/stream (SSE) — no-op if the
// endpoint is not yet wired.
//
// Backend endpoints (work-in-progress follow-up):
//   GET  /api/graph/stats     → { node_count, edge_count, ... }
//   GET  /api/graph/data      → { nodes:[{id,label,type}], edges:[{from,to,label}] }
//   GET  /api/graph/stream    → SSE emitting {type:"node"|"edge", ...}

const $ = (id) => document.getElementById(id);

async function fetchJson(path) {
  try {
    const r = await fetch(path, { headers: { 'accept': 'application/json' } });
    if (!r.ok) return null;
    return await r.json();
  } catch { return null; }
}

function renderNode(n) {
  const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  g.setAttribute('class', 'node');
  g.setAttribute('transform', `translate(${n.x ?? 0},${n.y ?? 0})`);
  const c = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
  c.setAttribute('r', 6);
  c.setAttribute('class', `dot ${(n.type || 'default').toLowerCase()}`);
  const t = document.createElementNS('http://www.w3.org/2000/svg', 'text');
  t.setAttribute('x', 10);
  t.setAttribute('y', 4);
  t.textContent = n.label || n.id;
  g.appendChild(c);
  g.appendChild(t);
  return g;
}

function renderEdge(e) {
  const l = document.createElementNS('http://www.w3.org/2000/svg', 'line');
  l.setAttribute('x1', e.x1 ?? 0);
  l.setAttribute('y1', e.y1 ?? 0);
  l.setAttribute('x2', e.x2 ?? 0);
  l.setAttribute('y2', e.y2 ?? 0);
  l.setAttribute('marker-end', 'url(#arrow)');
  l.setAttribute('class', 'edge');
  return l;
}

async function loadGraph() {
  const stats = await fetchJson('/api/graph/stats');
  $('stats').textContent = stats
    ? `${stats.node_count ?? '?'} nodes · ${stats.edge_count ?? '?'} edges`
    : 'graph api offline (see REMAINING.md G5 follow-up)';
  const data = await fetchJson('/api/graph/data?center=root&depth=2') || { nodes: [], edges: [] };
  const nodesLayer = $('nodes-layer');
  const edgesLayer = $('edges-layer');
  nodesLayer.replaceChildren();
  edgesLayer.replaceChildren();
  for (const n of data.nodes || []) nodesLayer.appendChild(renderNode(n));
  for (const e of data.edges || []) edgesLayer.appendChild(renderEdge(e));
}

$('search-btn').addEventListener('click', async () => {
  const q = $('q').value.trim();
  if (!q) return;
  const res = await fetchJson(`/api/graph/search?q=${encodeURIComponent(q)}`);
  const ul = $('results');
  ul.replaceChildren();
  for (const n of (res && res.nodes) || []) {
    const li = document.createElement('li');
    li.textContent = `${n.label || n.id} (${n.type || 'node'})`;
    li.addEventListener('click', () => { $('detail').textContent = JSON.stringify(n, null, 2); });
    ul.appendChild(li);
  }
});

$('show-stats').addEventListener('click', loadGraph);

loadGraph();

// SSE best-effort: if the endpoint isn't wired, just log it.
try {
  const es = new EventSource('/api/graph/stream');
  es.onmessage = (ev) => {
    const li = document.createElement('li');
    li.textContent = ev.data;
    $('stream-log').prepend(li);
  };
  es.onerror = () => es.close();
} catch (e) { /* expected until G5 follow-up lands */ }
