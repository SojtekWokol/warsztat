// EA2 PL workshop: lists of scenes, rooms and other texts, the names table and the line editor.
// Same structure as the P5R workshop. The data comes from the desktop app itself (Rust: warsztat-base +
// the ea2 adapter) through one Tauri command, api(route, d); pictures and fonts via http://zasob.localhost.
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const API = "http://zasob.localhost";
const call = (route, d) => window.__TAURI__.core.invoke("api", { route, d: d || {} }).catch((e) => ({ error: String(e) }));
// same call style as before: get("/api/page?key=…") and post("/api/save", {…})
const get = (url) => { const u = new URL(url, "http://x"); return call(u.pathname.replace(/^\/api\//, ""), Object.fromEntries(u.searchParams)); };
const post = (url, data) => call(url.replace(/^\/api\//, ""), data);
const debounce = (fn, ms) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; };
const ICON_PATHS = {
  pen: '<path d="M21.17 6.81a1 1 0 0 0-3.98-3.98L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z"/>',
  undo: '<path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"/>',
};
const icon = (n) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${ICON_PATHS[n]}</svg>`;
const LIMIT = 600;
const KIND = { ui: "Interfejs", quest: "Zadanie", item: "Przedmiot", puzzle: "Zagadka", hint: "Podpowiedź", text: "Tekst",
  dialog: "Scenka", vo: "Kwestia mówiona", name: "Postać", talk: "Rozmowa" };

let lists = { scenes: [], talks: [], rooms: [], campus: [], other: [] }, page = null, current = -1, ghost = "", typed = 0, mode = "scene";

// ---------- tabs ----------
document.querySelectorAll("#tabs button").forEach((b) => b.onclick = () => showTab(b.dataset.tab));
function showTab(tab) {
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("on", b.dataset.tab === tab));
  ["scenes", "talks", "rooms", "campus", "other", "names", "changes", "edit"].forEach((t) => $("pane-" + t).classList.toggle("hidden", t !== tab));
  $("help-card").classList.toggle("hidden", tab !== "edit");
  if (tab !== "edit" && typeof closeDrawer === "function") closeDrawer();
  if (tab === "changes") loadChanges();
}

// ---------- lists ----------
function table(head, rows, total) {
  const more = total > LIMIT ? `<p class="small more">Pokazano ${LIMIT} z ${total}. Zawęź wyszukiwanie.</p>` : "";
  return `<table><thead><tr>${head.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows}</tbody></table>${more}`;
}
function match(words, ...fields) {
  const hay = fields.join(" ").toLowerCase();
  return words.every((w) => hay.includes(w));
}
const prog = (p) => `<span class="prog"><i><b style="width:${(100 * p.done) / Math.max(1, p.total)}%"></b></i>${p.done}/${p.total}</span>`;
const doneFilter = (sel, p) => !sel || (sel === "yes" ? p.done === p.total : p.done < p.total);
const pen = (key) => `<button class="add" data-e="${esc(key)}" title="Edytuj">${icon("pen")}</button>`;

function renderScenes(which = "scenes") {
  const words = $("q-" + which).value.toLowerCase().split(/\s+/).filter(Boolean);
  const place = $("place-" + which).value, done = $("done-" + which).value;
  const list = lists[which].filter((s) => (!place || s.place === place) && doneFilter(done, s) && match(words, s.id, s.place, s.who.join(" "), s.start));
  $("t-" + which).innerHTML = table([which === "talks" ? "Rozmowa" : "Scena", "Miejsce", "Postacie", "Postęp", "Początek", ""], list.slice(0, LIMIT).map((s) =>
    `<tr data-key="${esc(s.key)}"><td class="id">${esc(s.id)}</td><td><span class="place">${esc(s.place)}</span></td>
      <td class="who-col" title="${esc(s.who.join(", "))}">${esc(s.who.join(", "))}</td><td>${prog(s)}</td>
      <td><div class="clamp">${esc(s.start)}</div></td><td>${pen(s.key)}</td></tr>`).join(""), list.length);
}
function renderAreas(which) {
  const words = $("q-" + which).value.toLowerCase().split(/\s+/).filter(Boolean);
  const done = $("done-" + which).value;
  const list = lists[which].filter((a) => doneFilter(done, a) && match(words, a.id, a.name, a.start));
  const kinds = (k) => Object.entries(k).sort((a, b) => b[1] - a[1]).slice(0, 3).map(([n, c]) => `${KIND[n] || n} ${c}`).join(", ");
  $("t-" + which).innerHTML = table(["Kod", "Nazwa", "Postęp", "Co jest w środku", "Początek", ""], list.slice(0, LIMIT).map((a) =>
    `<tr data-key="${esc(a.key)}"><td class="id">${esc(a.id)}</td><td>${esc(a.name)}</td><td>${prog(a)}</td>
      <td class="small">${esc(kinds(a.kinds))}</td><td><div class="clamp">${esc(a.start)}</div></td><td>${pen(a.key)}</td></tr>`).join(""), list.length);
}
document.addEventListener("click", (e) => {
  const ed = e.target.dataset?.e;
  if (ed) editPage(ed);
});
document.addEventListener("dblclick", (e) => {
  const tr = e.target.closest("tr[data-key]");
  if (tr) editPage(tr.dataset.key);
});
function editPage(key) { $("key").value = key; loadPage(key); showTab("edit"); }

// ---------- log and state ----------
function say(t) { const log = $("log"); log.textContent += t + "\n"; log.scrollTop = log.scrollHeight; }
function report(r) { if (r && r.error) say(r.error); }
let seen = 0, wasBuilding = false;
async function poll() {
  const s = await get("/api/state");
  if (s.error) throw new Error(s.error);
  $("bielik-state").innerHTML = s.model ? `<span class="ok-text">Bielik działa</span> (${esc(s.model)})` : `<span class="missing">Bielik nie działa.</span> <a href="#" id="bielik-how">Jak go włączyć?</a>`;
  $("bielik-how")?.addEventListener("click", (e) => { e.preventDefault(); $("bielik-help").classList.remove("hidden"); });
  $("edits").textContent = s.edits ? `Twoich zmian: ${s.edits} ▸ wgraj` : "Wszystko wgrane";
  $("edits").classList.toggle("has", s.edits > 0);
  $("bar-done").style.width = `${(100 * s.done) / s.total}%`;
  // the status counts open the matching lines in the editor
  const st = s.statuses || {};
  const pt = `${s.done} z ${s.total} · <a href="#" data-st="done">zatwierdzone ${st.done || 0}</a> · <a href="#" data-st="review">do sprawdzenia ${st.review || 0}</a> · <a href="#" data-st="draft">szkice ${st.draft || 0}</a>`;
  if ($("progress-text").innerHTML !== pt) $("progress-text").innerHTML = pt;
  if (s.log.length !== seen) { $("log").textContent = s.log.join("\n") + "\n"; $("log").scrollTop = 1e9; seen = s.log.length; }
  $("build").disabled = $("test").disabled = s.build.running;
  if (s.build.running || wasBuilding) buildOverlay(s, wasBuilding && !s.build.running);
  if (wasBuilding && !s.build.running) refreshLists();
  wasBuilding = s.build.running;
  return s;
}

// ---------- build overlay: blocks the workshop while the game files are written ----------
let buildStart = 0;
function buildOverlay(s, finished) {
  const box = $("building"), card = box.querySelector(".build-card");
  box.classList.remove("hidden");
  if (!buildStart) buildStart = Date.now();
  // lines of this run: everything after the last "Wgrywanie do gry..." entry
  const from = s.log.map((l) => l.includes("Wgrywanie do gry")).lastIndexOf(true);
  const lines = s.log.slice(Math.max(0, from));
  const stages = lines.map((l) => /\[(\d)\/4\]\s*(.*)/.exec(l)).filter(Boolean);
  const last = stages[stages.length - 1];
  const secs = Math.round((Date.now() - buildStart) / 1000);
  $("b-time").textContent = `Mija ${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
  if (!finished) {
    card.classList.remove("ok", "err");
    $("b-title").textContent = "Wgrywam do gry…";
    $("b-stage").textContent = last ? `Etap ${last[1]} z 4: ${last[2]}` : "Przygotowuję…";
    $("b-bar").style.width = `${last ? ((+last[1] - 0.5) / 4) * 100 : 3}%`;
    $("b-note").classList.remove("hidden");
    $("b-log").classList.add("hidden");
    $("b-ok").classList.add("hidden");
    return;
  }
  const ok = s.build.ok;
  card.classList.add(ok ? "ok" : "err");
  $("b-title").textContent = ok ? "Wgrano do gry" : "Wgrywanie nie powiodło się";
  $("b-stage").textContent = ok ? "Uruchom grę i wybierz w ustawieniach język POLSKI." : "Szczegóły poniżej. Gra musi być zamknięta podczas wgrywania.";
  $("b-bar").style.width = "100%";
  $("b-note").classList.add("hidden");
  $("b-log").textContent = lines.map((l) => l.replace(/^\d\d:\d\d\s+/, "")).join("\n");
  $("b-log").classList.toggle("hidden", ok);
  $("b-ok").classList.remove("hidden");
  $("b-ok").focus();
}
$("b-ok").onclick = () => { $("building").classList.add("hidden"); buildStart = 0; };
setInterval(() => poll().catch(() => {}), 1500);

// ---------- editor ----------
$("load").onclick = () => loadPage($("key").value);
$("key").addEventListener("keydown", (e) => { if (e.key === "Enter") loadPage($("key").value); });
async function loadPage(key, keep) {
  const r = await get("/api/page?key=" + encodeURIComponent(key.trim()));
  if (r.error) return say(r.error);
  page = r; $("key").value = r.key;
  $("edit-title").textContent = r.title;
  $("t-rows").innerHTML = table(["#", "Kto", "Angielski", "Polski (w grze)"], r.rows.map((x, n) =>
    `<tr data-n="${n}" class="${x.pl == null ? "none" : "user"} ${x.status === "review" ? "review" : x.status === "done" ? "done" : ""}">
      <td>${n + 1}</td><td title="${esc(x.who || x.key)}">${esc(x.who || (x.src === "y" ? "—" : x.key))}</td><td>${esc(x.en).replace(/\n|<br>/g, "<br>")}</td>
      <td class="pl">${x.pl == null ? "— brak —" : esc(x.pl).replace(/\n|<br>/g, "<br>")}</td></tr>`).join(""), r.rows.length);
  $("t-rows").querySelectorAll("tr[data-n]").forEach((tr) => tr.onclick = () => { pick(+tr.dataset.n); if (NARROW.matches) openDrawer(); });
  pick(Math.min(keep ?? 0, r.rows.length - 1));
}
function defaultMode(x) {
  if (x.kind === "dialog" || x.kind === "talk") return "scene";
  if (x.kind === "vo") return "sub";
  if (x.kind === "hint" || x.kind === "text") return x.en.length < 34 ? "button" : "note";
  if (x.kind === "puzzle" || x.kind === "name" || x.kind === "item") return "button";
  return x.en.length < 28 ? "button" : "menu";
}
function pick(n) {
  if (!page || !page.rows[n]) return;
  current = n;
  $("d-pos").textContent = `Kwestia ${n + 1} z ${page.rows.length}`;
  $("t-rows").querySelectorAll("tr[data-n]").forEach((tr) => tr.classList.toggle("sel", +tr.dataset.n === n));
  $("t-rows").querySelector(`tr[data-n="${n}"]`)?.scrollIntoView({ block: "nearest" });
  const x = page.rows[n];
  $("ed-key").textContent = x.key;
  $("ed-kind").textContent = KIND[x.kind] || x.kind;
  $("en").innerHTML = esc(x.en).replace(/&lt;.*?&gt;|\{[^}]+\}/g, (m) => `<span class="tag">${m}</span>`);
  $("who").textContent = x.who ? "Mówi: " + x.who : "";
  $("note").classList.toggle("hidden", !x.ctx);
  $("note-text").textContent = x.ctx || "";
  const refs = Object.entries(x.ref || {}).filter(([, v]) => v && v.trim());
  $("refs").classList.toggle("hidden", !refs.length);
  $("refs-list").innerHTML = refs.map(([l, v]) => `<dt>${l}</dt><dd>${esc(v)}</dd>`).join("");
  $("pl").value = x.pl ?? "";
  $("review").checked = x.status === "review";
  $("approved").checked = x.status === "done";
  mode = defaultMode(x);
  renderTabs();
  setGhost(""); check();
  $("variants").innerHTML = "";
  post("/api/glossary", { en: x.en }).then((g) => {
    $("gloss").innerHTML = g.terms.length
      ? g.terms.map((t) => `<p>${esc(t.en)} → ${t.pl ? `<b>${esc(t.pl)}</b>` : '<span class="missing">jeszcze bez tłumaczenia</span>'}` +
          `${t.draft ? ' <span class="missing">(propozycja)</span>' : ""}${t.note ? ` <span class="small">– ${esc(t.note)}</span>` : ""}</p>`).join("")
      : "<p>Brak terminów ze słownika ani nazw z gry w tej kwestii.</p>";
  });
}

// ---------- preview ----------
function rich(src) {
  let h = esc(src).replace(/\\n|\n/g, "<br>");
  h = h.replace(/&lt;style=(?:&quot;|")?([\w-]+)(?:&quot;|")?&gt;/gi, '<span class="st-$1">').replace(/&lt;\/style&gt;/gi, "</span>");
  h = h.replace(/&lt;color=(?:&quot;|")?(#[0-9a-f]{3,8}|[a-z]+)(?:&quot;|")?&gt;/gi, '<span style="color:$1">').replace(/&lt;\/color&gt;/gi, "</span>");
  h = h.replace(/&lt;size=(?:&quot;|")?([\d.]+)(%|em|px)?(?:&quot;|")?&gt;/gi, (m, n, u) => `<span style="font-size:${!u || u === "%" ? n + "%" : n + u}">`).replace(/&lt;\/size&gt;/gi, "</span>");
  h = h.replace(/&lt;mark=(?:&quot;|")?(#[0-9a-f]{3,8})(?:&quot;|")?&gt;/gi, '<span style="background:$1">').replace(/&lt;\/mark&gt;/gi, "</span>");
  h = h.replace(/&lt;link[^&]*&gt;/gi, '<span class="st-link">').replace(/&lt;\/link&gt;/gi, "</span>");
  h = h.replace(/&lt;mspace[^&]*&gt;/gi, '<span class="st-mono">').replace(/&lt;\/mspace&gt;/gi, "</span>");
  h = h.replace(/&lt;\/?align[^&]*&gt;/gi, "");
  h = h.replace(/&lt;(\/?)(b|i|u|s|sup|sub)&gt;/gi, "<$1$2>").replace(/&lt;br\s*\/?&gt;/gi, "<br>");
  h = h.replace(/&lt;.*?&gt;|\{[^}]+\}/g, (m) => `<span class="tag-chip">${m}</span>`);
  return h;
}
// Stage positions as the game draws them (checked against an in-game screenshot). FarLeft / FarRight are
// off-screen: characters wait there before walking in and leave there, so they are not drawn.
const POS = { farleft: null, left: 13, centerleft: 37, center: 50, middle: 50, centerright: 63, right: 87, farright: null };
const posOf = (p) => { const k = String(p || "").toLowerCase().replace(/[\s_-]/g, ""); return k in POS ? POS[k] : 50; };
const spriteUrl = (s) => `${API}/cache/vn/chars/${encodeURIComponent(s === "jeb_fullbody" ? "profjeb_fullbody" : s)}.png`;
function renderTabs() { document.querySelectorAll("#stage-tabs button").forEach((b) => b.classList.toggle("on", b.dataset.m === mode)); }
$("stage-tabs").onclick = (e) => { const b = e.target.closest("button"); if (b) { mode = b.dataset.m; renderTabs(); preview(); } };
const preview = debounce(() => {
  if (current < 0) return;
  const x = page.rows[current], typedPl = $("pl").value, t = rich(typedPl || x.en), who = esc(x.who || "");
  const stage = $("stage");
  stage.classList.toggle("en-only", !typedPl);
  const st = x.stage;
  let bg = "", chars = "";
  if (mode === "scene" && st) {
    if (st.bg) bg = `<div class="bg" style="background-image:url('${API}/cache/vn/bg/${encodeURIComponent(st.bg)}.jpg')"></div>`;
    // the speaking character in front and lit, the others dimmed behind, as in the game's dialogue scenes
    const speaker = (x.speaker || "").trim().toLowerCase();
    const isSpeaker = (c) => speaker && (c.name.toLowerCase() === speaker || speaker.endsWith(c.name.toLowerCase().split(" ").pop()));
    // some sprites named in the script were not shipped (e.g. tempest_uncool_fullbody): fall back to the
    // character's general sprite by first name (kira_fullbody), then drop the image
    const fallback = (c) => spriteUrl(`${c.name.split(" ")[0].toLowerCase()}_fullbody`);
    chars = `<div class="chars">${(st.chars || []).filter((c) => posOf(c.pos) !== null).map((c) => `<img class="${isSpeaker(c) ? "talking" : speaker ? "quiet" : ""}" src="${spriteUrl(c.sprite)}" data-alt="${fallback(c)}" style="left:${posOf(c.pos)}%" alt="${esc(c.name)}" title="${esc(c.name)}" onerror="if(this.dataset.alt&&this.src!==this.dataset.alt){this.src=this.dataset.alt;this.dataset.alt=''}else{this.remove()}">`).join("")}</div>`;
  }
  const body = {
    scene: `<div class="g-dialog">${who ? `<div class="name">${who}</div>` : ""}<div class="box2">${t}</div></div>`,
    button: `<div class="g-button checker"><div class="t">${t}</div></div>`,
    menu: `<div class="g-menu"><div class="h">${t}</div><div class="r"><span>Master Volume</span><span class="slider"></span></div></div>`,
    note: `<div class="g-note">${t}</div>`,
    sub: `<div class="g-sub">${who ? `<span class="whosub">${who}:</span> ` : ""}${t}</div>`,
  }[mode];
  stage.innerHTML = bg + chars + body;
}, 60);
const check = debounce(async () => {
  if (current < 0) return;
  preview();
  const x = page.rows[current];
  const r = await post("/api/check", { en: x.en, pl: $("pl").value, id: x.id });
  const n = $("pl").value.length, m = x.en.length;
  $("check").textContent = `długość ${n} / oryginał ${m}` + (r.problems.length ? "  |  " + r.problems.join("; ") : "  |  OK");
  $("check").className = "check " + (r.problems.length ? "bad" : "ok");
  // tag differences can be accepted as deliberate (e.g. added emphasis for a pun); saved texts only
  if (r.tag_problems && x.pl != null) {
    const b = document.createElement("button");
    b.className = "tags-ok";
    b.textContent = r.tags_ok ? "znaczniki celowo inne ✓ (cofnij)" : "Celowo inne";
    b.title = r.tags_ok ? "Kontrola znaczników znowu obejmie tę kwestię" : "Oznacz różnicę w znacznikach jako świadomą: kontrola przestanie ją zgłaszać, dopóki znaczniki się nie zmienią";
    b.onclick = async () => { await post("/api/tags_ok", { id: x.id, pl: $("pl").value, ok: !r.tags_ok }); check(); };
    $("check").append(" ", b);
  }
}, 150);

// ---------- tag ribbon: the original's <tags> and {codes}, click to insert (wraps a selection) ----------
const TAG_RX = /<[^<>]+>|\{[^{}]+\}/g;
const normTag = (t) => t.startsWith("{") ? t : t.replace(/^<\s*(\/?)\s*([\w-]+)\s*(?:=\s*["']?([^"'>]*?)["']?\s*)?>$/, (m, c, n, v) => `<${c}${n.toLowerCase()}${v ? "=" + v.toLowerCase() : ""}>`);
const countTags = (s) => (s.match(TAG_RX) || []).map(normTag).reduce((a, t) => (a[t] = (a[t] || 0) + 1, a), {});
function closerOf(t) { const m = t.match(/^<\s*([\w-]+)/); return m && !t.startsWith("</") && !/^<(br|sprite|space|page)/i.test(t) ? `</${m[1]}>` : null; }
function tagbar() {
  const bar = $("tagbar");
  if (current < 0 || !page) return (bar.innerHTML = "");
  const en = page.rows[current].en, want = countTags(en), have = countTags($("pl").value);
  const seen = new Set(), chips = [];
  for (const t of en.match(TAG_RX) || []) {
    const k = normTag(t);
    if (seen.has(k) || k.startsWith("</")) continue;
    seen.add(k);
    const close = closerOf(t), ck = close && normTag(close);
    const ok = (have[k] || 0) >= want[k] && (!ck || (have[ck] || 0) >= (want[ck] || 0));
    const n = want[k] > 1 ? ` <i>×${want[k]}</i>` : "";
    chips.push(`<button type="button" class="tchip ${ok ? "ok" : "miss"}" data-t="${esc(t)}" title="${close ? "Zaznacz słowo i kliknij, żeby je objąć znacznikiem" : "Wstaw w miejscu kursora"}${ok ? "" : " (brakuje w tłumaczeniu)"}">${esc(t)}${close ? `…${esc(close)}` : ""}${n}</button>`);
  }
  const extra = Object.keys(have).filter((k) => (have[k] || 0) > (want[k] || 0));
  bar.innerHTML = chips.length || extra.length
    ? `<span class="tlabel">Znaczniki</span>${chips.join("")}` + (extra.length ? `<span class="textra" title="Tych znaczników jest w tłumaczeniu więcej niż w oryginale">nadmiar: ${esc(extra.join(" "))}</span>` : "")
    : "";
  bar.classList.toggle("hidden", !bar.innerHTML);
  bar.querySelectorAll(".tchip").forEach((b) => b.onmousedown = (e) => { e.preventDefault(); insertTag(b.dataset.t); });
}
function insertTag(t) {
  const ta = $("pl"), a = ta.selectionStart, b = ta.selectionEnd, close = closerOf(t);
  const sel = ta.value.slice(a, b), ins = close ? t + sel + close : t;
  ta.focus();
  ta.setRangeText(ins, a, b, "end");
  if (close && !sel) ta.selectionStart = ta.selectionEnd = a + t.length;  // caret between the pair
  typed++; setGhost(""); check();
}
$("tag-audit").onclick = () => loadPage("!znaczniki");
$("progress-text").onclick = (e) => { const a = e.target.closest("a[data-st]"); if (!a) return; e.preventDefault(); editPage("!status:" + a.dataset.st); };
$("bielik-info").onclick = () => $("bielik-help").classList.remove("hidden");
$("bh-close").onclick = () => $("bielik-help").classList.add("hidden");
$("bielik-help").onclick = (e) => { if (e.target.id === "bielik-help") $("bielik-help").classList.add("hidden"); };
// "approved" and "to check" exclude each other
$("approved").onchange = () => { if ($("approved").checked) $("review").checked = false; };
$("review").onchange = () => { if ($("review").checked) $("approved").checked = false; };
$("approve-page").onclick = async () => {
  if (!page) return;
  const ids = page.rows.filter((x) => x.pl != null && x.status === "draft").map((x) => x.id);
  if (!ids.length) return say("Na tej stronie nie ma szkiców do zatwierdzenia.");
  if (!confirm(`Zatwierdzić ${ids.length} kwestii na tej stronie?`)) return;
  report(await post("/api/approve", { ids }));
  loadPage(page.key, current);
};

// Grey suggestion drawn in a mirror layer under the transparent textarea, only when the caret is at the end.
function setGhost(text) {
  ghost = text || "";
  $("mirror").innerHTML = esc($("pl").value).replace(/&lt;[^&]*?&gt;|\{[^}]*\}/g, (m) => `<span class="mtag">${m}</span>`) +
    `<span class="ghost">${esc(ghost)}</span>` + "​";
  tagbar();
}
function atEnd() { const t = $("pl"); return t.selectionStart === t.value.length && t.selectionEnd === t.value.length; }
const wordGhost = debounce(async () => {
  if (!atEnd()) return;
  const m = $("pl").value.match(/([^\s\d.,!?;:"'()\-–]{2,})$/u);
  if (!m) return;
  const stamp = typed;
  const r = await get("/api/word?part=" + encodeURIComponent(m[1]));
  if (stamp === typed && r.rest) setGhost(r.rest);
}, 120);
const llmGhost = debounce(async () => {
  if (current < 0 || !atEnd() || $("pl").value.trim().length < 3) return;
  const stamp = typed, x = page.rows[current];
  const r = await post("/api/continue", { prefix: $("pl").value, id: x.id }).catch(() => ({}));
  if (stamp === typed && atEnd() && r.text) setGhost(r.text);
}, 900);
$("pl").addEventListener("input", () => { typed++; setGhost(""); check(); wordGhost(); llmGhost(); });
$("pl").addEventListener("scroll", () => { $("mirror").scrollTop = $("pl").scrollTop; });
$("pl").addEventListener("keydown", (e) => {
  if (e.key === "Tab") {
    e.preventDefault();
    if (ghost) { $("pl").value += ghost; typed++; setGhost(""); check(); }
  } else if (e.key === "Enter" && e.ctrlKey) {
    e.preventDefault(); save(true);
  } else if (!["Shift", "Control", "Alt"].includes(e.key)) {
    if (ghost) setGhost("");
  }
});
$("pl").addEventListener("click", () => { if (ghost && !atEnd()) setGhost(""); });

async function save(next) {
  if (current < 0) return;
  const x = page.rows[current];
  const r = await post("/api/save", { id: x.id, pl: $("pl").value, status: $("approved").checked ? "done" : $("review").checked ? "review" : "draft", everywhere: $("everywhere").checked });
  if (r.error) return say(r.error);
  poll();
  await loadPage(page.key, next ? Math.min(current + 1, page.rows.length - 1) : current);
}
$("save").onclick = () => save(false);
$("restore").onclick = async () => {
  if (current < 0) return;
  await post("/api/restore", { id: page.rows[current].id });
  poll();
  loadPage(page.key, current);
};
const build = async () => {
  const r = await post("/api/build", {});
  if (r.error) return report(r);
  buildStart = Date.now();
  wasBuilding = true; // show the overlay at once, before the next poll sees the run
  buildOverlay({ log: [], build: { running: true } }, false);
  poll();
};
$("build").onclick = build;
$("test").onclick = build;
$("edits").onclick = build;
$("launch").onclick = async () => { report(await post("/api/launch", {})); poll(); };

$("suggest").onclick = async () => {
  if (current < 0) return;
  const x = page.rows[current];
  const context = page.rows.slice(Math.max(0, current - 4), current).map((p) => `${p.who || "-"}: ${(p.pl || p.en).replace(/\n/g, " ")}`);
  $("variants").innerHTML = "<li>Bielik myśli... (pierwsze zapytanie trwa do minuty)</li>";
  const r = await post("/api/suggest", { id: x.id, pl: $("pl").value, context, hint: $("hint").value });
  $("variants").innerHTML = (r.variants || []).map((v) => `<li>${esc(v)}</li>`).join("") || `<li>${esc(r.error || "Brak wariantów.")}</li>`;
  $("variants").querySelectorAll("li").forEach((li) => li.onclick = () => {
    if (!r.variants) return;
    $("pl").value = li.textContent; typed++; setGhost(""); check();
  });
};
$("mem-q").addEventListener("input", debounce(async () => {
  const r = await get("/api/memory?q=" + encodeURIComponent($("mem-q").value));
  $("mem").innerHTML = r.hits.map(([en, pl]) => `<div class="pair">EN: ${esc(en)}<br><b>PL: ${esc(pl)}</b></div>`).join("") +
    (r.total ? `<p>${r.total} wyników${r.total > 60 ? ", pokazane najkrótsze 60" : ""}</p>` : "");
}, 250));

// ---------- names ----------
const GROUP_PL = { speaker: "Mówiący w dialogach", name: "Postacie", item: "Przedmioty", quest: "Zadania", text: "Nazwy miejsc", ui: "Interfejs", hint: "Inne", puzzle: "Inne" };
let names = [];
async function loadNames() {
  names = await get("/api/names");
  const gs = [...new Set(names.map((n) => n.group))].sort((a, b) => (GROUP_PL[a] || a).localeCompare(GROUP_PL[b] || b, "pl"));
  $("grp-names").innerHTML = `<option value="">Wszystkie rodzaje</option>` + gs.map((g) => `<option value="${esc(g)}">${esc(GROUP_PL[g] || g)}</option>`).join("");
  renderNames();
}
function renderNames() {
  const words = $("q-names").value.toLowerCase().split(/\s+/).filter(Boolean);
  const g = $("grp-names").value, done = $("done-names").value;
  const list = names.map((n, i) => [n, i]).filter(([n]) => !g || n.group === g)
    .filter(([n]) => !done || (done === "yes" ? n.pl : !n.pl))
    .filter(([n]) => match(words, n.en, n.pl, n.key, n.area));
  $("t-names").innerHTML = table(["Rodzaj", "Miejsce", "Angielski", "Polski", ""],
    list.slice(0, LIMIT).map(([n, i]) => `<tr data-n="${i}" class="${n.pl ? "mine" : ""}"><td class="small">${esc(GROUP_PL[n.group] || n.group)}</td>
      <td class="small">${esc(n.area)}</td><td>${esc(n.en)}</td>
      <td><input class="name-pl" value="${esc(n.pl)}" placeholder="${esc(n.en)}" data-n="${i}"></td>
      <td>${n.pl ? `<button class="add" data-undo="${i}" title="Usuń tłumaczenie">${icon("undo")}</button>` : ""}</td></tr>`).join(""), list.length);
}
async function nameSave(inp, text) {
  const i = +inp.dataset.n, n = names[i];
  const r = await post("/api/name_save", { id: n.id, pl: text });
  if (r.error) return say(r.error);
  n.pl = text;
  poll();
  const tr = inp.closest("tr");
  tr.classList.toggle("mine", !!text);
  tr.lastElementChild.innerHTML = text ? `<button class="add" data-undo="${i}" title="Usuń tłumaczenie">${icon("undo")}</button>` : "";
  if (!text) inp.value = "";
  else tr.nextElementSibling?.querySelector(".name-pl")?.focus();
}
document.addEventListener("keydown", (e) => {
  const inp = e.target;
  if (!inp.classList?.contains("name-pl")) return;
  if (e.key === "Enter") { e.preventDefault(); if (inp.value !== names[inp.dataset.n].pl) nameSave(inp, inp.value); }
  if (e.key === "Escape") { inp.value = names[inp.dataset.n].pl; inp.blur(); }
});
document.addEventListener("click", (e) => {
  const u = e.target.dataset?.undo;
  if (u !== undefined) nameSave(document.querySelector(`input[data-n="${u}"]`), "");
});

// ---------- start ----------
async function refreshLists() {
  const [scenes, talks, rooms, campus, other] = await Promise.all(["scenes", "talks", "rooms", "campus", "other"].map((t) => get("/api/pages?tab=" + t)));
  lists = { scenes, talks, rooms, campus, other };
  renderScenes("scenes"); renderScenes("talks"); ["rooms", "campus", "other"].forEach(renderAreas);
}
(async () => {
  // the data loads together with the window; if it cannot, say why
  for (let i = 0; ; i++) {
    try { await poll(); break; } catch (e) {
      if (i === 2) $("offline").classList.remove("hidden");
      if (e && e.message) $("offline-msg").textContent = e.message;
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
  $("offline").classList.add("hidden");
  await refreshLists();
  ["scenes", "talks"].forEach((w) => {
    const places = [...new Set(lists[w].map((s) => s.place))].sort((a, b) => a.localeCompare(b, "pl"));
    $("place-" + w).innerHTML += places.map((p) => `<option>${esc(p)}</option>`).join("");
    $("q-" + w).addEventListener("input", debounce(() => renderScenes(w), 150));
    $("place-" + w).onchange = () => renderScenes(w);
    $("done-" + w).onchange = () => renderScenes(w);
  });
  ["rooms", "campus", "other"].forEach((w) => {
    $("q-" + w).addEventListener("input", debounce(() => renderAreas(w), 150));
    $("done-" + w).onchange = () => renderAreas(w);
  });
  $("q-names").addEventListener("input", debounce(renderNames, 150));
  $("grp-names").onchange = renderNames;
  $("done-names").onchange = renderNames;
  loadNames();
})();

// ---------- window buttons (desktop app only) ----------
if (window.__TAURI__) {
  document.body.classList.add("app");
  const w = window.__TAURI__.window.getCurrentWindow();
  $("w-min").onclick = () => w.minimize();
  $("w-max").onclick = () => w.toggleMaximize();
  $("w-close").onclick = () => w.close();
  document.querySelector("header").addEventListener("dblclick", (e) => { if (e.target.closest("[data-tauri-drag-region]") === e.target) w.toggleMaximize(); });
}

// ---------- narrow screens: the editor opens as a drawer over the right side ----------
const NARROW = matchMedia("(max-width: 1599px)");
const editorHome = document.querySelector(".editor"), helpHome = $("test-card");
function placeEditor() {
  const narrow = NARROW.matches;
  document.body.classList.toggle("narrow", narrow);
  if (narrow) {
    $("drawer-body").append($("editbox"), $("help-card"));
  } else {
    editorHome.append($("editbox"));
    helpHome.after($("help-card"));
    closeDrawer();
  }
}
function openDrawer() { $("drawer").classList.remove("hidden"); $("drawer-bg").classList.remove("hidden"); document.body.classList.add("drawer-open"); $("pl").focus(); }
function closeDrawer() { $("drawer").classList.add("hidden"); $("drawer-bg").classList.add("hidden"); document.body.classList.remove("drawer-open"); }
$("drawer-bg").onclick = () => closeDrawer();
NARROW.addEventListener("change", placeEditor);
placeEditor();
$("d-close").onclick = closeDrawer;
$("d-prev").onclick = () => pick(current - 1);
$("d-next").onclick = () => pick(current + 1);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("drawer").classList.contains("hidden")) closeDrawer();
  if (e.ctrlKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && page) { e.preventDefault(); pick(current + (e.key === "ArrowUp" ? -1 : 1)); }
});

// ---------- corrections files ----------
const STATUS = { nowa: "nowa", zmiana: "zmiana", konflikt: "konflikt", jest: "już jest", "zła": "zła" };
const PICK = (st) => st === "nowa" || st === "zmiana"; // ticked by default
let importItems = [];
const lsGet = (k) => { try { return localStorage.getItem(k) || ""; } catch { return ""; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, v); } catch { /* storage may be blocked */ } };
$("corr-author").value = lsGet("author");
$("corr-author").oninput = () => lsSet("author", $("corr-author").value.trim());
$("corr-export").onclick = async () => report(await post("/api/corr_export", { author: $("corr-author").value.trim() }));
$("corr-import").onclick = () => $("corr-file").click();
$("corr-file").onchange = async () => {
  const f = $("corr-file").files[0];
  $("corr-file").value = "";
  if (!f) return;
  let payload;
  try { payload = JSON.parse(await f.text()); } catch { return say("To nie jest plik z poprawkami."); }
  const r = await post("/api/corr_review", payload);
  if (r.error) return say(r.error);
  importItems = r.items;
  const n = (st) => r.items.filter((x) => x.status === st).length;
  $("imp-head").textContent = `${r.author || "Ktoś bez pseudonimu"}${r.created ? ", " + r.created : ""}: ${r.items.length} poprawek ` +
    `(zmiany ${n("zmiana")}, nowe ${n("nowa")}, konflikty ${n("konflikt")}, już są ${n("jest")}, złe ${n("zła")}). ` +
    (r.base ? `Pomocnik zaczynał od wersji ${r.base}${r.base_known ? "" : " (której nie masz – konflikty mogą być na wyrost)"}. ` : "") +
    `„Zmiana” to kwestia, której od tamtej wersji nie ruszałeś; „konflikt” – zmienialiście ją obaj. Zaznaczone trafią do tłumaczenia ze statusem „do sprawdzenia”.`;
  $("imp-list").innerHTML = r.items.map((x) => {
    const usable = x.status === "nowa" || x.status === "zmiana" || x.status === "konflikt";
    return `<label class="imp-item ${usable ? "" : "off"}">
      <input type="checkbox" data-id="${esc(x.id)}" ${PICK(x.status) ? "checked" : ""} ${usable ? "" : "disabled"}>
      <span class="imp-status ${x.status === "zła" ? "zla" : x.status}">${STATUS[x.status] || x.status}</span>
      <span><div class="imp-text"><span><b>Angielski</b>${esc(x.en)}</span><span><b>Teraz</b>${esc(x.current ?? "(brak tłumaczenia)")}</span>${x.status === "konflikt" && x.was != null ? `<span><b>Pomocnik widział</b>${esc(x.was || "(brak)")}</span>` : ""}
        <span class="new"><b>Propozycja</b>${esc(x.pl)}</span></div></span>
      ${x.problems.length ? `<span></span><span></span><span class="imp-problems">${esc(x.problems.join("; "))}</span>` : ""}
    </label>`;
  }).join("");
  importCount();
  $("import").classList.remove("hidden");
};
function importCount() {
  const n = document.querySelectorAll("#imp-list input:checked").length;
  $("imp-count").textContent = `zaznaczono ${n}`;
  $("imp-accept").disabled = !n;
}
$("imp-list").addEventListener("change", importCount);
$("imp-all").onchange = () => {
  document.querySelectorAll("#imp-list input:not(:disabled)").forEach((c) => {
    const it = importItems.find((x) => x.id === c.dataset.id);
    if (it && PICK(it.status)) c.checked = $("imp-all").checked;
  });
  importCount();
};
$("imp-cancel").onclick = () => $("import").classList.add("hidden");
$("imp-accept").onclick = async () => {
  const ids = [...document.querySelectorAll("#imp-list input:checked")].map((c) => c.dataset.id);
  const r = await post("/api/corr_accept", { ids, items: importItems });
  if (r.error) return say(r.error);
  $("import").classList.add("hidden");
  poll(); refreshLists(); loadNames();
  if (page) loadPage(page.key, current);
};


// ---------- changes since the last release, split into in the game (wgrane) and not yet (niewgrane) ----------
let changes = null, chgMode = "all";
const when = (t) => t ? new Date(t * 1000).toLocaleString("pl-PL", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "—";
const nextVersion = (v) => { if (!v) return "1.0.0"; const p = v.split(".").map(Number); p[2] += 1; return p.join("."); };
async function loadChanges() {
  changes = await get("/api/changes");
  $("rel-version").placeholder = nextVersion(changes.release && changes.release.version);
  renderChanges();
}
function renderChanges() {
  if (!changes) return;
  const all = changes.items, pending = all.filter((c) => !c.in_game);
  const list = chgMode === "pending" ? pending : chgMode === "ingame" ? all.filter((c) => c.in_game) : all;
  const words = $("q-changes").value.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = list.filter((c) => match(words, c.key, c.en, c.old, c.new));
  const r = changes.release;
  $("chg-info").textContent = r
    ? `Zmiany od wersji ${r.version} (wydana ${when(r.time)}): ${all.length}, z tego wgrane ${all.length - pending.length}, niewgrane ${pending.length}. Ostatnie wgranie: ${when(changes.last_build)}.`
    : `Nie ma jeszcze żadnego wydania. Widać tylko zmiany niewgrane od ostatniego wgrania (${when(changes.last_build)}). Wydaj wersję, żeby śledzić zmiany od niej.`;
  const mark = (c) => `<span class="place ${c.in_game ? "in" : "out"}">${c.in_game ? "wgrane" : "niewgrane"}</span>`;
  const kind = (c) => !c.old ? "nowe" : !c.new ? "usunięte" : "zmienione";
  $("t-changes").innerHTML = shown.length ? table(["Stan", "Klucz", "Angielski", r ? `W wersji ${esc(r.version)}` : "W grze", "Teraz", ""], shown.slice(0, LIMIT).map((c) =>
    `<tr data-key="${esc(c.id)}"><td>${mark(c)}<div class="small">${kind(c)}</div></td><td class="id">${esc(c.key)}</td><td><div class="clamp">${esc(c.en)}</div></td>
      <td class="small"><div class="clamp">${esc(c.old) || "—"}</div></td><td class="pl"><div class="clamp">${esc(c.new) || "—"}</div></td>
      <td>${pen(c.id)}</td></tr>`).join(""), shown.length)
    : `<p class="more">${chgMode === "pending" ? "Nic nie czeka na wgranie: gra ma wszystkie Twoje zmiany." : "Brak zmian od wydania."}</p>`;
}
$("chg-mode").onclick = (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  chgMode = b.dataset.m;
  document.querySelectorAll("#chg-mode button").forEach((x) => x.classList.toggle("on", x === b));
  renderChanges();
};
$("q-changes").addEventListener("input", debounce(renderChanges, 150));
$("rel-make").onclick = async () => {
  const v = ($("rel-version").value || $("rel-version").placeholder).trim();
  const pending = changes ? changes.items.filter((c) => !c.in_game).length : 0;
  if (pending && !confirm(`${pending} zmian nie jest jeszcze wgranych i nie wejdzie do wersji ${v} (wydanie to stan wgrany do gry). Wydać mimo to?`)) return;
  const r = await post("/api/release", { version: v });
  if (r.error) return say(r.error);
  $("rel-version").value = "";
  poll(); loadChanges();
};

// tabs that do not fit in a narrow window scroll sideways with the mouse wheel
document.querySelector("nav").addEventListener("wheel", (e) => {
  const nav = e.currentTarget;
  if (nav.scrollWidth > nav.clientWidth && Math.abs(e.deltaY) > Math.abs(e.deltaX)) { nav.scrollLeft += e.deltaY; e.preventDefault(); }
}, { passive: false });
