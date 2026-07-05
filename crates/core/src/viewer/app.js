// MemPalace Dashboard SPA — embedded at compile time.
//
// Read-only viewer: fetches data from the REST API, never sends mutations.
// Force-directed graph is a minimal inline physics sim (no D3 dependency).
// SSE stream provides live updates from the backend.

const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

async function fetchJson(path) {
  try {
    const r = await fetch(path, { headers: { accept: "application/json" } });
    if (!r.ok) return null;
    return await r.json();
  } catch {
    return null;
  }
}

async function postJson(path, body) {
  try {
    const r = await fetch(path, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json" },
      body: JSON.stringify(body),
    });
    if (!r.ok) return null;
    return await r.json();
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Dashboard stats
// ---------------------------------------------------------------------------

async function loadDashboard() {
  // Palace status — drawer/wing/room counts
  const status = await fetchJson("/status");
  if (status) {
    const drawers = status.total_drawers ?? status.drawer_count ?? "--";
    const wings = status.wings ?? status.wing_count ?? "--";
    const rooms = status.rooms ?? status.room_count ?? "--";
    $("val-drawers").textContent = drawers;
    $("val-wings").textContent = wings;
    $("val-rooms").textContent = rooms;
  }

  // Knowledge graph entity count
  const kg = await fetchJson("/kg/stats");
  if (kg) {
    $("val-entities").textContent = kg.total_entities ?? kg.entity_count ?? "--";
  }

  // Top bar status
  const graphStats = await fetchJson("/graph/stats");
  if (graphStats) {
    const parts = [];
    if (graphStats.total_rooms != null) parts.push(graphStats.total_rooms + " rooms");
    if (graphStats.total_edges != null) parts.push(graphStats.total_edges + " tunnels");
    $("stats").textContent = parts.length ? parts.join(" · ") : "palace connected";
    $("stats").className = "stats ok";
  } else {
    $("stats").textContent = "backend offline";
    $("stats").className = "stats err";
  }
}

// ---------------------------------------------------------------------------
// Recent observations
// ---------------------------------------------------------------------------

async function loadObservations() {
  const data = await fetchJson("/working_memory");
  const ul = $("observations");
  ul.replaceChildren();
  if (!data) {
    ul.innerHTML = '<li class="muted">no observations available</li>';
    return;
  }
  const items = data.observations ?? data.items ?? (Array.isArray(data) ? data : []);
  for (const obs of items.slice(0, 20)) {
    const li = document.createElement("li");
    const hook = document.createElement("span");
    hook.className = "obs-hook";
    hook.textContent = obs.hook_type ?? obs.type ?? "";
    const text = document.createElement("span");
    text.textContent = (obs.content ?? obs.text ?? "").slice(0, 80);
    li.appendChild(hook);
    li.appendChild(text);
    li.addEventListener("click", () => {
      $("detail").innerHTML = `<h4>Observation</h4><pre style="white-space:pre-wrap;font-size:11px;">${escHtml(JSON.stringify(obs, null, 2))}</pre>`;
    });
    ul.appendChild(li);
  }
  if (items.length === 0) {
    ul.innerHTML = '<li class="muted">no observations</li>';
  }
}

// ---------------------------------------------------------------------------
// Force-directed graph (minimal inline physics)
// ---------------------------------------------------------------------------

const graphNodes = new Map();  // id -> {id, label, type, x, y, vx, vy, el}
const graphEdges = [];         // {from, to, label, el}
let selectedNodeId = null;
let svgWidth = 800;
let svgHeight = 600;
let dragging = null;
let dragOffX = 0;
let dragOffY = 0;

function nodeColor(type) {
  switch ((type || "").toLowerCase()) {
    case "entity": return "#79c0ff";
    case "concept": return "#d2a8ff";
    case "file": return "#ffa657";
    case "person": return "#f0883e";
    case "relation": return "#56d4dd";
    default: return "#8b95a1";
  }
}

function addGraphNode(id, label, type) {
  if (graphNodes.has(id)) return graphNodes.get(id);
  const angle = Math.random() * Math.PI * 2;
  const r = 50 + Math.random() * 100;
  const node = {
    id,
    label: label || id,
    type: type || "default",
    x: svgWidth / 2 + Math.cos(angle) * r,
    y: svgHeight / 2 + Math.sin(angle) * r,
    vx: 0,
    vy: 0,
    el: null,
  };
  // Create SVG group
  const ns = "http://www.w3.org/2000/svg";
  const g = document.createElementNS(ns, "g");
  g.setAttribute("class", "node");
  const c = document.createElementNS(ns, "circle");
  c.setAttribute("r", "6");
  c.setAttribute("fill", nodeColor(type));
  const t = document.createElementNS(ns, "text");
  t.setAttribute("x", "10");
  t.setAttribute("y", "4");
  t.textContent = node.label;
  g.appendChild(c);
  g.appendChild(t);

  // Drag handling
  g.addEventListener("mousedown", (ev) => {
    ev.preventDefault();
    dragging = node;
    const pt = svgPoint(ev);
    dragOffX = pt.x - node.x;
    dragOffY = pt.y - node.y;
  });

  // Click for detail
  g.addEventListener("click", (ev) => {
    if (ev.detail === 2) return; // ignore double-click drag
    selectNode(node.id);
  });

  node.el = g;
  $("nodes-layer").appendChild(g);
  graphNodes.set(id, node);
  return node;
}

function addGraphEdge(fromId, toId, label) {
  // Avoid duplicate edges
  for (const e of graphEdges) {
    if (e.from === fromId && e.to === toId) return;
  }
  const ns = "http://www.w3.org/2000/svg";
  const line = document.createElementNS(ns, "line");
  line.setAttribute("marker-end", "url(#arrow)");
  line.setAttribute("class", "edge");
  if (label) {
    const txt = document.createElementNS(ns, "text");
    txt.textContent = label;
    txt.setAttribute("class", "edge");
    $("edges-layer").appendChild(txt);
  }
  $("edges-layer").appendChild(line);
  graphEdges.push({ from: fromId, to: toId, label: label || "", el: line });
}

function clearGraph() {
  graphNodes.clear();
  graphEdges.length = 0;
  $("nodes-layer").replaceChildren();
  $("edges-layer").replaceChildren();
  selectedNodeId = null;
}

function svgPoint(ev) {
  const svg = $("graph");
  const rect = svg.getBoundingClientRect();
  return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
}

// Simple force simulation tick
function tick() {
  const nodeArr = [...graphNodes.values()];
  const N = nodeArr.length;
  if (N === 0) return;

  // Repulsion between all pairs
  for (let i = 0; i < N; i++) {
    for (let j = i + 1; j < N; j++) {
      const a = nodeArr[i], b = nodeArr[j];
      let dx = b.x - a.x, dy = b.y - a.y;
      let dist = Math.sqrt(dx * dx + dy * dy) || 1;
      let force = 800 / (dist * dist);
      let fx = (dx / dist) * force;
      let fy = (dy / dist) * force;
      a.vx -= fx; a.vy -= fy;
      b.vx += fx; b.vy += fy;
    }
  }

  // Attraction along edges
  for (const e of graphEdges) {
    const a = graphNodes.get(e.from);
    const b = graphNodes.get(e.to);
    if (!a || !b) continue;
    let dx = b.x - a.x, dy = b.y - a.y;
    let dist = Math.sqrt(dx * dx + dy * dy) || 1;
    let force = (dist - 80) * 0.01;
    let fx = (dx / dist) * force;
    let fy = (dy / dist) * force;
    a.vx += fx; a.vy += fy;
    b.vx -= fx; b.vy -= fy;
  }

  // Center gravity
  for (const n of nodeArr) {
    n.vx += (svgWidth / 2 - n.x) * 0.001;
    n.vy += (svgHeight / 2 - n.y) * 0.001;
    // Damping
    n.vx *= 0.85;
    n.vy *= 0.85;
    // Update position (skip dragged node)
    if (n !== dragging) {
      n.x += n.vx;
      n.y += n.vy;
      // Bounds
      n.x = Math.max(20, Math.min(svgWidth - 20, n.x));
      n.y = Math.max(20, Math.min(svgHeight - 20, n.y));
    }
    // Update SVG
    n.el.setAttribute("transform", `translate(${n.x},${n.y})`);
  }

  // Update edges
  for (const e of graphEdges) {
    const a = graphNodes.get(e.from);
    const b = graphNodes.get(e.to);
    if (!a || !b) continue;
    e.el.setAttribute("x1", a.x);
    e.el.setAttribute("y1", a.y);
    e.el.setAttribute("x2", b.x);
    e.el.setAttribute("y2", b.y);
  }
}

// Run simulation until settled
let simFrame = 0;
function runSimulation() {
  simFrame = 0;
  function step() {
    tick();
    simFrame++;
    if (simFrame < 300) requestAnimationFrame(step);
  }
  step();
}

// Mouse events for drag
document.addEventListener("mousemove", (ev) => {
  if (!dragging) return;
  const pt = svgPoint(ev);
  dragging.x = pt.x - dragOffX;
  dragging.y = pt.y - dragOffY;
  dragging.el.setAttribute("transform", `translate(${dragging.x},${dragging.y})`);
});
document.addEventListener("mouseup", () => { dragging = null; });

// ---------------------------------------------------------------------------
// Graph data loading
// ---------------------------------------------------------------------------

async function loadGraph() {
  // Try graph/stats first for basic info
  const stats = await fetchJson("/graph/stats");
  if (stats && stats.rooms_per_wing) {
    // Build graph from rooms_per_wing data
    clearGraph();
    for (const [wing, rooms] of Object.entries(stats.rooms_per_wing)) {
      addGraphNode(wing, wing, "entity");
      if (Array.isArray(rooms)) {
        for (const room of rooms) {
          const rid = typeof room === "string" ? room : room.name || room.id;
          addGraphNode(rid, rid, "concept");
          addGraphEdge(wing, rid, "contains");
        }
      }
    }
    // Add top tunnels as edges between wings
    if (stats.top_tunnels) {
      for (const t of stats.top_tunnels) {
        const from = t.source ?? t.from;
        const to = t.target ?? t.to;
        if (from && to) {
          addGraphNode(from, from, "entity");
          addGraphNode(to, to, "entity");
          addGraphEdge(from, to, t.predicate || t.label || "tunnel");
        }
      }
    }
    runSimulation();
    return;
  }

  // Fallback: try graph/search with broad query
  const searchResult = await postJson("/graph/search", { entity_names: [], depth: 2, limit: 50 });
  if (searchResult && searchResult.status === "ok") {
    clearGraph();
    for (const ent of searchResult.entities || []) {
      const id = ent.name ?? ent.id;
      addGraphNode(id, id, ent.type || "entity");
    }
    for (const rel of searchResult.relationships || []) {
      addGraphEdge(rel.subject, rel.object, rel.predicate);
    }
    runSimulation();
  }
}

// ---------------------------------------------------------------------------
// Node detail
// ---------------------------------------------------------------------------

function selectNode(id) {
  const node = graphNodes.get(id);
  if (!node) return;
  selectedNodeId = id;

  // Highlight selected
  for (const n of graphNodes.values()) {
    const circle = n.el.querySelector("circle");
    circle.classList.toggle("selected", n.id === id);
  }

  // Find connected edges
  const connected = [];
  for (const e of graphEdges) {
    if (e.from === id || e.to === id) {
      connected.push(e);
    }
  }

  let html = `<h4>${escHtml(node.label)}</h4>`;
  html += `<p class="type">${escHtml(node.type)}</p>`;
  if (connected.length) {
    html += "<ul>";
    for (const e of connected) {
      const other = e.from === id ? e.to : e.from;
      const dir = e.from === id ? "→" : "←";
      html += `<li><b>${escHtml(e.label || "link")}</b> ${dir} ${escHtml(other)}</li>`;
    }
    html += "</ul>";
  } else {
    html += '<p class="muted">no connections</p>';
  }
  $("detail").innerHTML = html;
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

$("search-btn").addEventListener("click", doSearch);
$("q").addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") doSearch();
});

async function doSearch() {
  const q = $("q").value.trim();
  if (!q) return;

  // Use graph/search with the query as entity name
  const res = await postJson("/graph/search", { entity_names: [q], depth: 2, limit: 20 });
  const ul = $("results");
  ul.replaceChildren();

  if (res && res.status === "ok") {
    const entities = res.entities || [];
    for (const ent of entities) {
      const id = ent.name ?? ent.id;
      const li = document.createElement("li");
      li.innerHTML = `<span class="name">${escHtml(id)}</span><span class="meta">${escHtml(ent.type || "entity")}</span>`;
      li.addEventListener("click", () => {
        selectNode(id);
        // Also add to graph if not present
        addGraphNode(id, id, ent.type || "entity");
        for (const rel of res.relationships || []) {
          if (rel.subject === id || rel.object === id) {
            addGraphNode(rel.subject, rel.subject, "entity");
            addGraphNode(rel.object, rel.object, "entity");
            addGraphEdge(rel.subject, rel.object, rel.predicate);
          }
        }
        runSimulation();
      });
      ul.appendChild(li);
    }
    if (entities.length === 0) {
      ul.innerHTML = '<li class="muted">no results</li>';
    }
  } else {
    ul.innerHTML = '<li class="muted">search failed</li>';
  }
}

// Refresh graph button
$("show-graph").addEventListener("click", loadGraph);

// ---------------------------------------------------------------------------
// SSE live stream
// ---------------------------------------------------------------------------

function connectSSE() {
  try {
    const es = new EventSource("/sse");
    es.onopen = () => {
      logStream("connected to SSE", "event");
    };
    es.onmessage = (ev) => {
      logStream(ev.data, "");
    };
    es.addEventListener("node", (ev) => {
      logStream("node: " + ev.data, "node");
    });
    es.addEventListener("edge", (ev) => {
      logStream("edge: " + ev.data, "edge");
    });
    es.addEventListener("observation", (ev) => {
      logStream("obs: " + ev.data, "event");
      // Refresh observations list on new activity
      loadObservations();
    });
    es.onerror = () => {
      logStream("SSE disconnected", "err");
      es.close();
      // Reconnect after 10s
      setTimeout(connectSSE, 10000);
    };
  } catch {
    // SSE not available
  }
}

function logStream(text, cls) {
  const ul = $("stream-log");
  const li = document.createElement("li");
  if (cls) li.className = cls;
  li.textContent = text;
  ul.prepend(li);
  // Keep only 50 entries
  while (ul.children.length > 50) ul.removeChild(ul.lastChild);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function escHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---------------------------------------------------------------------------
// SVG resize observer
// ---------------------------------------------------------------------------

function updateSvgSize() {
  const svg = $("graph");
  const rect = svg.parentElement.getBoundingClientRect();
  svgWidth = rect.width || 800;
  svgHeight = rect.height || 600;
  svg.setAttribute("viewBox", `0 0 ${svgWidth} ${svgHeight}`);
}

const resizeObs = new ResizeObserver(() => {
  updateSvgSize();
  tick(); // re-layout
});
resizeObs.observe($("graph").parentElement);

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

async function init() {
  updateSvgSize();
  await Promise.all([loadDashboard(), loadGraph(), loadObservations()]);
  connectSSE();
}

init();
