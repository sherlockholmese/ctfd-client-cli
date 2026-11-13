// ctfdc live scoreboard page.
//
// Receives full board snapshots from the local ctfdc server over
// server-sent events and renders them.  All text from the server (team
// and challenge names are chosen by CTF players) goes through textContent
// or escapeHtml, never raw innerHTML.
"use strict";

(() => {
  // ------------------------------------------------------------ setup

  const params = new URLSearchParams(location.hash.slice(1));
  const KEY = params.get("k") || "";
  const $ = (id) => document.getElementById(id);
  const fmt = new Intl.NumberFormat();
  const num = (n) => fmt.format(n);
  const signed = (n) => (n > 0 ? "+" : n < 0 ? "−" : "") + num(Math.abs(n));

  const state = {
    snap: null,
    view: ["analysis", "plan"].includes(params.get("view")) ? params.get("view") : "board",
    range: "all",
    offset: 0, // server clock minus local clock, seconds
    receivedAt: 0,
    connected: false,
    everConnected: false,
    hidden: new Set(), // series keys hidden via the legend
    linked: null, // series key highlighted from a table row
    refreshing: false,
    goal: null, // ladder key picked as the Plan goal; null follows the server's
  };

  // ------------------------------------------------------------ theme

  // Series colors: a fixed, CVD-checked order of eight hues stepped for
  // each surface.  Teams past the eighth line fold to a neutral.
  const THEMES = {
    light: {
      series: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"],
      overflow: "#a3a3a3",
      fg: "#0a0a0a",
      muted: "#737373",
      grid: "#ececec",
      axis: "#d4d4d4",
      card: "#ffffff",
      popover: "#ffffff",
      border: "#e5e5e5",
    },
    dark: {
      series: ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#2f9e2f", "#9085e9", "#e66767"],
      overflow: "#5c5c5c",
      fg: "#fafafa",
      muted: "#a1a1a1",
      grid: "#262626",
      axis: "#3a3a3a",
      card: "#111111",
      popover: "#171717",
      border: "#2e2e2e",
    },
  };

  const media = window.matchMedia("(prefers-color-scheme: dark)");
  function storedTheme() {
    try {
      return localStorage.getItem("ctfdc-theme");
    } catch {
      return null;
    }
  }
  function themeName() {
    const forced = document.documentElement.dataset.theme;
    if (forced === "light" || forced === "dark") return forced;
    return media.matches ? "dark" : "light";
  }
  function palette() {
    return THEMES[themeName()];
  }
  function applyTheme(name) {
    if (name) document.documentElement.dataset.theme = name;
    else delete document.documentElement.dataset.theme;
    const next = themeName() === "dark" ? "light" : "dark";
    $("theme").setAttribute("aria-label", `Switch to ${next} theme`);
  }
  applyTheme(storedTheme());
  $("theme").addEventListener("click", () => {
    const next = themeName() === "dark" ? "light" : "dark";
    applyTheme(next);
    try {
      localStorage.setItem("ctfdc-theme", next);
    } catch {
      /* private mode: the choice lasts for this page only */
    }
    rerenderCharts();
  });
  media.addEventListener("change", () => {
    applyTheme(document.documentElement.dataset.theme);
    rerenderCharts();
  });

  // Color follows the team, not its rank: a key keeps its slot while it
  // stays graphed, so rank swaps never repaint lines.
  const slots = new Map();
  function assignSlots(lines) {
    const live = new Set(lines.filter((l) => !l.is_me).map((l) => l.key));
    for (const key of [...slots.keys()]) if (!live.has(key)) slots.delete(key);
    const used = new Set(slots.values());
    for (const l of lines) {
      if (l.is_me || slots.has(l.key)) continue;
      let s = 0;
      while (used.has(s)) s++;
      slots.set(l.key, s);
      used.add(s);
    }
  }
  function colorOf(key, isMe) {
    const p = palette();
    if (isMe) return p.fg;
    const s = slots.get(key);
    if (s === undefined) return null;
    return s < p.series.length ? p.series[s] : p.overflow;
  }

  // ------------------------------------------------------------ helpers

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  }

  function el(tag, attrs, ...children) {
    const node = document.createElement(tag);
    if (attrs) {
      for (const [k, v] of Object.entries(attrs)) {
        if (v === null || v === undefined || v === false) continue;
        if (k === "class") node.className = v;
        else if (k === "text") node.textContent = v;
        else if (k === "style") node.style.cssText = v;
        else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
        else node.setAttribute(k, v === true ? "" : v);
      }
    }
    for (const c of children) {
      if (c === null || c === undefined || c === false) continue;
      node.append(c instanceof Node ? c : document.createTextNode(String(c)));
    }
    return node;
  }

  function icon(name) {
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", "icon");
    svg.setAttribute("aria-hidden", "true");
    const use = document.createElementNS(ns, "use");
    use.setAttribute("href", `#i-${name}`);
    svg.append(use);
    return svg;
  }

  function serverNow() {
    return Date.now() / 1000 + state.offset;
  }

  function ago(t) {
    const s = Math.max(0, Math.round(serverNow() - t));
    if (s < 45) return "now";
    if (s < 3600) return `${Math.round(s / 60)}m`;
    if (s < 86400) {
      const h = Math.floor(s / 3600);
      const m = Math.round((s % 3600) / 60);
      return m ? `${h}h ${m}m` : `${h}h`;
    }
    return `${Math.floor(s / 86400)}d`;
  }

  function agoLong(t) {
    const a = ago(t);
    return a === "now" ? "just now" : `${a} ago`;
  }

  function clock(t) {
    return new Date(t * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  // Absent entries (null, false) are skipped rather than printed.
  function replace(parent, nodes) {
    parent.replaceChildren(...nodes.filter((n) => n !== null && n !== undefined && n !== false));
  }

  async function api(path, body) {
    const res = await fetch(`api/${path}`, {
      method: body === undefined ? "GET" : "POST",
      headers: { "x-ctfdc-key": KEY, "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`${res.status}`);
    return res;
  }

  // ------------------------------------------------------------ tabs

  const tabs = [$("tab-board"), $("tab-analysis"), $("tab-plan")];
  function showView(view, focus) {
    state.view = view;
    if (view !== "analysis") closeCombo(false);
    for (const t of tabs) {
      const on = t.id === `tab-${view}`;
      t.setAttribute("aria-selected", on);
      t.tabIndex = on ? 0 : -1;
      if (on && focus) t.focus();
    }
    $("view-board").hidden = view !== "board";
    $("view-analysis").hidden = view !== "analysis";
    $("view-plan").hidden = view !== "plan";
    const p = new URLSearchParams(location.hash.slice(1));
    if (view === "board") p.delete("view");
    else p.set("view", view);
    history.replaceState(null, "", `#${p}`);
    if (state.snap) render();
    requestAnimationFrame(() => {
      progress.chart?.resize();
      radar.chart?.resize();
    });
  }
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => showView(t.id.slice(4)));
    t.addEventListener("keydown", (e) => {
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      e.preventDefault();
      const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
      showView(next.id.slice(4), true);
    });
  });

  // Time range (radio group).
  const ranges = [...$("range").querySelectorAll(".toggle")];
  function setRange(btn, focus) {
    state.range = btn.dataset.range;
    for (const b of ranges) {
      const on = b === btn;
      b.setAttribute("aria-checked", on);
      b.tabIndex = on ? 0 : -1;
    }
    if (focus) btn.focus();
    renderProgress();
  }
  ranges.forEach((b, i) => {
    b.addEventListener("click", () => setRange(b));
    b.addEventListener("keydown", (e) => {
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      e.preventDefault();
      setRange(ranges[(i + (e.key === "ArrowRight" ? 1 : ranges.length - 1)) % ranges.length], true);
    });
  });

  // ------------------------------------------------------------ status

  function renderStatus() {
    const s = state.snap;
    const status = $("status");
    let mode = "live";
    let text = "";
    if (!state.connected && state.everConnected) {
      mode = "offline";
      text = "Disconnected";
    } else if (!s) {
      mode = "refreshing";
      text = "Connecting…";
    } else if (s.error) {
      mode = "error";
      text = "Update failed";
    } else {
      // Like the terminal view: when the data is from, and when it next updates.
      const next = Math.ceil(s.updated_at + s.interval - serverNow());
      text = s.refreshing || next <= 0
        ? `Updated ${clock(s.updated_at)}, updating…`
        : `Updated ${clock(s.updated_at)}, next in ${next}s`;
    }
    status.dataset.state = mode;
    status.textContent = text;

    const alert = $("alert");
    if (mode === "offline") {
      alert.hidden = false;
      $("alert-title").textContent = "Lost connection to ctfdc";
      $("alert-body").textContent = s
        ? `Showing data from ${clock(s.updated_at)}`
        : "";
    } else if (mode === "error") {
      alert.hidden = false;
      $("alert-title").textContent = "Couldn't update the scoreboard";
      $("alert-body").textContent = `${s.error}. Showing data from ${clock(s.updated_at)}`;
    } else {
      alert.hidden = true;
    }
  }

  const refreshBtn = $("refresh");
  refreshBtn.addEventListener("click", async () => {
    refreshBtn.setAttribute("aria-busy", "true");
    state.refreshing = true;
    try {
      await api("refresh", {});
    } catch {
      refreshBtn.removeAttribute("aria-busy");
    }
    setTimeout(() => refreshBtn.removeAttribute("aria-busy"), 10000);
  });

  // ------------------------------------------------------------ header

  function renderHeader() {
    const s = state.snap;
    $("host").textContent = s.host;
    $("host").title = s.host;
    $("demo-badge").hidden = !s.demo;
    const since = $("since-badge");
    since.hidden = !s.since;
    if (s.since) {
      const d = new Date(s.since * 1000);
      since.textContent = `Since ${d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}`;
      since.title = "Points scored before this time are ignored";
    }
    document.title = `${s.host} scoreboard`;
    $("th-name").textContent = s.noun === "team" ? "Team" : "Player";
    $("search").placeholder = `Find a ${s.noun}`;
    $("search").previousElementSibling.textContent = `Find a ${s.noun}`;
    const top = `Top ${s.top_n}`;
    $("th-top").textContent = top;
    $("th-field-top").textContent = top;
  }

  // ------------------------------------------------------------ charts

  function baseText() {
    const p = palette();
    return { color: p.muted, fontFamily: getComputedStyle(document.body).fontFamily, fontSize: 12 };
  }

  function tooltipFrame() {
    const p = palette();
    return {
      backgroundColor: p.popover,
      borderColor: p.border,
      borderWidth: 1,
      padding: 0,
      textStyle: { color: p.fg },
      extraCssText:
        "border-radius:8px;box-shadow:0 10px 15px -3px rgb(0 0 0 / .12),0 4px 6px -4px rgb(0 0 0 / .1);",
      confine: true,
      transitionDuration: 0.1,
    };
  }

  function makeChart(node) {
    const chart = echarts.init(node, null, { renderer: "svg" });
    new ResizeObserver(() => chart.resize()).observe(node);
    return chart;
  }

  // Score of a step line at time t (seconds).
  function scoreAt(points, t) {
    let lo = 0;
    let hi = points.length - 1;
    if (hi < 0 || t < points[0][0]) return 0;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (points[mid][0] <= t) lo = mid;
      else hi = mid - 1;
    }
    return points[lo][1];
  }

  const progress = { chart: null };

  function renderProgress() {
    const s = state.snap;
    if (!s || state.view !== "board") return;
    const lines = s.lines;
    const hasData = lines.some((l) => l.points.length > 2);
    $("progress-empty").hidden = hasData;
    $("progress-chart").hidden = !hasData;
    renderLegend();
    if (!hasData) return;
    if (typeof echarts === "undefined") return;
    if (!progress.chart) {
      progress.chart = makeChart($("progress-chart"));
      progress.chart.on("mouseover", { seriesType: "line" }, (e) => linkRow(e.seriesId, true));
      progress.chart.on("mouseout", { seriesType: "line" }, (e) => linkRow(e.seriesId, false));
    }

    const p = palette();
    const now = s.now;
    const first = Math.min(...lines.map((l) => (l.points.length ? l.points[0][0] : now)));
    const start = state.range === "all" ? first : Math.max(first, now - Number(state.range));
    const visible = lines.filter((l) => !state.hidden.has(l.key));
    let yMin = 0;
    if (state.range !== "all" && visible.length) {
      yMin = Math.min(...visible.map((l) => scoreAt(l.points, start)));
    }
    const yMax = Math.max(1, ...visible.map((l) => scoreAt(l.points, now)));
    if (yMin > 0) yMin = Math.max(0, Math.floor((yMin - (yMax - yMin) * 0.08) / 50) * 50);

    // You last, so your line draws on top.
    const ordered = [...lines].sort((a, b) => Number(a.is_me) - Number(b.is_me));
    const series = ordered.map((l) => {
      const color = colorOf(l.key, l.is_me);
      const overflow = !l.is_me && color === p.overflow;
      return {
        id: l.key,
        name: l.name,
        type: "line",
        data: state.hidden.has(l.key) ? [] : l.points.map(([t, v]) => [t * 1000, v]),
        showSymbol: false,
        symbol: "circle",
        symbolSize: 7,
        clip: true,
        z: l.is_me ? 5 : overflow ? 1 : 2,
        color,
        lineStyle: {
          width: l.is_me ? 2.5 : overflow ? 1.25 : 2,
          color,
          opacity: overflow ? 0.8 : 1,
          cap: "round",
          join: "round",
        },
        areaStyle: l.is_me
          ? {
              color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                { offset: 0, color: hexAlpha(p.fg, themeName() === "dark" ? 0.14 : 0.08) },
                { offset: 1, color: hexAlpha(p.fg, 0) },
              ]),
              origin: "start",
            }
          : undefined,
        endLabel: l.is_me
          ? {
              show: true,
              formatter: () => "You",
              color: p.fg,
              fontWeight: 600,
              fontSize: 12,
              distance: 8,
              fontFamily: baseText().fontFamily,
            }
          : undefined,
        emphasis: { focus: "series", lineStyle: { width: l.is_me ? 3 : 2.5 } },
        blur: { lineStyle: { opacity: 0.12 }, areaStyle: { opacity: 0.2 }, label: { opacity: 0.3 } },
        triggerLineEvent: true,
      };
    });

    progress.chart.setOption(
      {
        animationDuration: 500,
        animationDurationUpdate: 300,
        animationEasingUpdate: "cubicOut",
        textStyle: baseText(),
        grid: { left: 8, right: 96, top: 16, bottom: 8, containLabel: true },
        xAxis: {
          type: "time",
          min: start * 1000,
          max: now * 1000,
          axisLine: { lineStyle: { color: p.axis } },
          axisTick: { show: false },
          axisLabel: {
            color: p.muted,
            hideOverlap: true,
            margin: 12,
            formatter: {
              year: "{yyyy}",
              month: "{MMM}",
              day: "{d} {MMM}",
              hour: "{HH}:{mm}",
              minute: "{HH}:{mm}",
              second: "{HH}:{mm}:{ss}",
              millisecond: "{HH}:{mm}:{ss}",
              none: "{HH}:{mm}",
            },
          },
          splitLine: { show: false },
          axisPointer: {
            show: true,
            type: "line",
            lineStyle: { color: p.muted, type: "solid", width: 1, opacity: 0.5 },
            label: { show: false },
          },
        },
        yAxis: {
          type: "value",
          min: yMin,
          max: (v) => niceMax(v.max),
          splitNumber: 4,
          axisLine: { show: false },
          axisTick: { show: false },
          axisLabel: { color: p.muted, formatter: (v) => num(v), margin: 12 },
          splitLine: { lineStyle: { color: p.grid } },
        },
        tooltip: {
          ...tooltipFrame(),
          trigger: "axis",
          axisPointer: { type: "line", animation: false },
          formatter: (items) => progressTip(items),
        },
        series,
      },
      { replaceMerge: ["series"] },
    );
  }

  function niceMax(v) {
    const padded = v * 1.08;
    const mag = 10 ** Math.floor(Math.log10(Math.max(1, padded)));
    return Math.ceil(padded / (mag / 2)) * (mag / 2);
  }

  function progressTip(items) {
    const s = state.snap;
    if (!items.length) return "";
    const t = items[0].axisValue / 1000;
    const rows = s.lines
      .filter((l) => !state.hidden.has(l.key))
      .map((l) => ({ l, v: scoreAt(l.points, t) }))
      .sort((a, b) => b.v - a.v || Number(b.l.is_me) - Number(a.l.is_me));
    const max = 10;
    let shown = rows.slice(0, max);
    const me = rows.find((r) => r.l.is_me);
    if (me && !shown.includes(me)) shown = [...shown.slice(0, max - 1), me];
    const d = new Date(t * 1000);
    const title = d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
    const html = shown
      .map(({ l, v }) => {
        const color = colorOf(l.key, l.is_me);
        const name = l.is_me ? `${l.name} (you)` : l.name;
        return `<div class="chart-tip-row${l.is_me ? " is-me" : ""}"><span class="bar" style="background:${color}"></span><span class="name">${escapeHtml(name)}</span><span class="val">${num(v)}</span></div>`;
      })
      .join("");
    const more = rows.length - shown.length;
    return `<div class="chart-tip"><div class="chart-tip-title">${escapeHtml(title)}</div><div class="chart-tip-rows">${html}</div>${more > 0 ? `<div class="chart-tip-more">+${more} more</div>` : ""}</div>`;
  }

  function hexAlpha(hex, a) {
    const n = parseInt(hex.slice(1), 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }

  function renderLegend() {
    const s = state.snap;
    const items = [...s.lines]
      .sort((a, b) => Number(b.is_me) - Number(a.is_me))
      .map((l) => {
        const on = !state.hidden.has(l.key);
        const btn = el(
          "button",
          {
            class: "legend-item",
            type: "button",
            "aria-pressed": on ? "true" : "false",
            title: on ? `Hide ${l.name}` : `Show ${l.name}`,
            onclick: () => {
              if (state.hidden.has(l.key)) state.hidden.delete(l.key);
              else state.hidden.add(l.key);
              renderProgress();
            },
            onmouseenter: () => highlight(l.key, true),
            onmouseleave: () => highlight(l.key, false),
            onfocus: () => highlight(l.key, true),
            onblur: () => highlight(l.key, false),
          },
          el("span", { class: "swatch", style: `background:${colorOf(l.key, l.is_me)}` }),
          el("span", { text: l.is_me ? `${l.name} (you)` : l.name }),
        );
        return el("li", null, btn);
      });
    replace($("legend"), items);
  }

  function highlight(key, on) {
    if (!progress.chart || state.hidden.has(key)) return;
    if (!state.snap.lines.some((l) => l.key === key)) return;
    progress.chart.dispatchAction({ type: on ? "highlight" : "downplay", seriesId: key });
  }

  function linkRow(key, on) {
    const row = document.querySelector(`#standings-body tr[data-key="${CSS.escape(key)}"]`);
    if (row) row.classList.toggle("is-linked", on);
  }

  // Points by category as a radar: each spoke is the share of that
  // category's points you (and the rival) hold, so big and small
  // categories read on the same scale. The tooltip carries the raw numbers.
  const radar = { chart: null };

  function renderCategoryChart() {
    const h = state.snap.h2h;
    const rival = h.rival;
    const theirsKnown = !!rival && !h.rival_loading;
    const cats = h.categories
      .filter((c) => c.total_points > 0)
      .sort((a, b) => b.total_points - a.total_points || a.category.localeCompare(b.category));
    const node = $("cat-chart");
    const enough = cats.length >= 3;
    $("cat-empty").hidden = enough;
    $("cat-empty").textContent = cats.length ? "Needs at least three categories" : "Nothing yet";
    node.hidden = !enough;
    if (!enough || typeof echarts === "undefined") return;
    if (!radar.chart) radar.chart = makeChart(node);

    const p = palette();
    const meColor = p.fg;
    const rivalColor = p.series[0];
    const share = (pts, c) => (c.total_points > 0 ? Math.round((pts / c.total_points) * 1000) / 10 : 0);
    const label = (c) => c.category || "Uncategorized";

    node.setAttribute(
      "aria-label",
      cats
        .map((c) => `${label(c)}: you ${num(c.mine_points)}${theirsKnown ? `, ${rival.name} ${num(c.theirs_points)}` : ""} of ${num(c.total_points)}`)
        .join("; "),
    );
    // The table's column heads double as the legend.
    $("swatch-me").style.background = meColor;
    $("swatch-rival").style.background = rivalColor;
    $("swatch-rival").hidden = !theirsKnown;

    const tip = () => {
      const head = `<tr><th>Category</th><th>You</th>${theirsKnown ? `<th>${escapeHtml(rival.name)}</th><th>Gap</th>` : "<th>Of</th>"}</tr>`;
      const rows = [...cats]
        .map((c) => ({ c, gap: c.mine_points - c.theirs_points }))
        .sort((x, y) => (theirsKnown ? x.gap - y.gap : 0) || y.c.total_points - x.c.total_points)
        .map(({ c, gap }) => {
          const mine = `${num(c.mine_points)}<span class="sub">${c.mine}/${c.total}</span>`;
          const rest = theirsKnown
            ? `<td>${num(c.theirs_points)}<span class="sub">${c.theirs}/${c.total}</span></td><td class="${gap > 0 ? "delta-up" : gap < 0 ? "delta-down" : "muted"}">${signed(gap) || "0"}</td>`
            : `<td class="muted">${num(c.total_points)}</td>`;
          return `<tr><td>${escapeHtml(label(c))}</td><td>${mine}</td>${rest}</tr>`;
        })
        .join("");
      return `<div class="chart-tip chart-tip-wide"><table class="chart-tip-table">${head}${rows}</table></div>`;
    };

    const area = (color, alpha) => ({ color: hexAlpha(color, alpha) });
    const series = [];
    if (theirsKnown) {
      series.push({
        name: rival.name,
        value: cats.map((c) => share(c.theirs_points, c)),
        symbol: "circle",
        symbolSize: 5,
        itemStyle: { color: rivalColor },
        lineStyle: { color: rivalColor, width: 2 },
        areaStyle: area(rivalColor, 0.18),
      });
    }
    series.push({
      name: "You",
      value: cats.map((c) => share(c.mine_points, c)),
      symbol: "circle",
      symbolSize: 5,
      z: 3,
      itemStyle: { color: meColor },
      lineStyle: { color: meColor, width: 2.5 },
      areaStyle: area(meColor, themeName() === "dark" ? 0.14 : 0.08),
    });

    radar.chart.setOption(
      {
        animationDuration: 500,
        animationDurationUpdate: 300,
        textStyle: baseText(),
        tooltip: { ...tooltipFrame(), trigger: "item", formatter: tip },
        radar: {
          center: ["50%", "52%"],
          radius: "68%",
          startAngle: 90,
          splitNumber: 4,
          shape: "polygon",
          indicator: cats.map((c) => ({ name: label(c), max: 100, min: 0 })),
          axisName: { color: p.muted, fontSize: 12, fontFamily: baseText().fontFamily },
          axisNameGap: 8,
          axisLine: { lineStyle: { color: p.axis } },
          splitLine: { lineStyle: { color: p.grid } },
          splitArea: { show: false },
        },
        series: [{ type: "radar", data: series, emphasis: { lineStyle: { width: 3 } } }],
      },
      { notMerge: true },
    );
  }

  function rerenderCharts() {
    if (!state.snap) return;
    renderProgress();
    renderStandings();
    if (state.view === "analysis") renderAnalysis();
  }

  // ------------------------------------------------------------ standings

  function standingsRow(r, prev) {
    const color = r.slot !== null ? colorOf(r.key, r.is_me) : null;
    const behind = prev ? prev.score - r.score : null;
    const name = el("span", { class: "team-name", text: r.name, title: r.name });
    const team = el(
      "div",
      { class: "team" },
      el("span", {
        class: color ? "swatch" : "swatch swatch-empty",
        style: color ? `background:${color}` : null,
        title: color ? "On the graph" : null,
        "aria-hidden": "true",
      }),
      name,
      r.is_me ? el("span", { class: "badge badge-solid badge-sm", text: "You" }) : null,
      r.is_rival ? el("span", { class: "badge badge-outline badge-sm", text: "Rival" }) : null,
    );

    let change = null;
    if (r.change) {
      const c = r.change;
      change = el("span", { class: "delta" });
      if (c.rank > 0) change.append(el("span", { class: "delta-up", text: `▲${c.rank}`, title: `Up ${c.rank}` }));
      else if (c.rank < 0) change.append(el("span", { class: "delta-down", text: `▼${-c.rank}`, title: `Down ${-c.rank}` }));
      if (c.score) change.append(el("span", { class: "delta-pts", text: signed(c.score) }));
    }

    const actions = el("div", { class: "row-actions" });
    if (r.id !== null && (r.can_pin || r.pinned)) {
      actions.append(
        el(
          "button",
          {
            class: "btn btn-ghost btn-sm btn-icon btn-pin",
            type: "button",
            "aria-pressed": r.pinned ? "true" : "false",
            "aria-label": r.pinned ? `Remove ${r.name} from the graph` : `Add ${r.name} to the graph`,
            title: r.pinned ? "Remove from graph" : "Add to graph",
            onclick: (e) => pin(r, e.currentTarget),
          },
          icon("pin"),
        ),
      );
    }
    if (!r.is_me) {
      actions.append(
        el("button", {
          class: "btn btn-ghost btn-sm btn-compare",
          type: "button",
          text: "Compare",
          "aria-label": `Compare with ${r.name}`,
          onclick: () => compare(r.key),
        }),
      );
    }

    const tr = el(
      "tr",
      {
        "data-key": r.key,
        class: r.is_me ? "is-me" : null,
        "aria-current": r.is_me ? "true" : null,
        onmouseenter: () => highlight(r.key, true),
        onmouseleave: () => highlight(r.key, false),
      },
      el("td", { class: `num rank${r.pos <= 3 ? " rank-podium" : ""}`, text: r.pos }),
      el("td", { class: "col-name" }, team),
      el("td", { class: "num score", text: num(r.score) }),
      el("td", { class: "num hide-sm col-behind muted", text: behind === null ? "" : behind === 0 ? "Tied" : num(behind) }),
      el("td", { class: "num hide-sm col-move" }, change),
      el("td", null, actions),
    );
    return tr;
  }

  function renderStandings() {
    const s = state.snap;
    const q = $("search").value.trim().toLowerCase();
    const rows = [];
    let shown = 0;
    s.standings.forEach((r, i) => {
      if (s.cutoff && i === s.cutoff && !q) {
        rows.push(el("tr", { class: "cutoff", "aria-hidden": "true" }, el("td", { colspan: 6 }, el("span", { text: `Top ${s.cutoff} qualify` }))));
      }
      if (q && !r.name.toLowerCase().includes(q)) return;
      shown++;
      rows.push(standingsRow(r, s.standings[i - 1]));
    });
    replace($("standings-body"), rows);
    const empty = $("standings-empty");
    empty.hidden = shown > 0;
    empty.textContent = s.standings.length ? "No match" : "Nobody ranked yet";
    $("jump-me").hidden = !s.standings.some((r) => r.is_me);
    if (state.linked) linkRow(state.linked, true);
  }

  $("search").addEventListener("input", () => state.snap && renderStandings());
  $("search").addEventListener("keydown", (e) => {
    if (e.key === "Escape" && e.currentTarget.value) {
      e.currentTarget.value = "";
      renderStandings();
    }
  });
  $("jump-me").addEventListener("click", () => {
    if ($("search").value) {
      $("search").value = "";
      renderStandings();
    }
    const row = document.querySelector("#standings-body tr.is-me");
    if (!row) return;
    row.scrollIntoView({ block: "center", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  });

  async function pin(r, btn) {
    btn.disabled = true;
    try {
      await api("pin", { id: r.id, pinned: !r.pinned });
    } catch {
      btn.disabled = false;
    }
  }

  async function compare(key) {
    showView("analysis");
    try {
      await api("rival", { key });
    } catch {
      /* the board stays on the previous rival */
    }
  }

  // ------------------------------------------------------------ feed

  function renderFeed() {
    const s = state.snap;
    const items = s.feed.map((f) =>
      el(
        "li",
        { class: f.is_me ? "is-me" : null },
        el("span", { class: "feed-time", text: ago(f.t), title: new Date(f.t * 1000).toLocaleString(), "data-t": f.t }),
        el(
          "div",
          { class: "feed-main" },
          el("span", { class: "feed-team" }, el("span", { text: f.team, title: f.team }), f.is_me ? el("span", { class: "badge badge-solid badge-sm", text: "You" }) : null, f.fresh ? el("span", { class: "badge badge-outline badge-sm", text: "New" }) : null),
          // Category/Name, as the challenge folders and `ctfdc s` spell it.
          el(
            "span",
            { class: "feed-chal", title: f.category ? `${f.category}/${f.challenge}` : f.challenge },
            f.category ? el("span", { class: "feed-cat", text: f.category }) : null,
            f.category ? el("span", { class: "feed-sep", text: "/", "aria-hidden": "true" }) : null,
            el("span", { text: f.challenge }),
          ),
        ),
        el("span", { class: "feed-pts", text: signed(f.value) }),
      ),
    );
    replace($("feed"), items);
    $("feed-empty").hidden = s.feed.length > 0;
  }

  // ------------------------------------------------------------ analysis

  // Rival picker: a searchable listbox in a popover (a shadcn-style
  // combobox).  Focus stays in the search field; arrow keys move the
  // highlighted option, aria-activedescendant tells assistive tech.
  const combo = {
    trigger: $("rival"),
    pop: $("rival-pop"),
    search: $("rival-search"),
    list: $("rival-list"),
    none: $("rival-none"),
    items: [],
    active: -1,
    pending: null, // key chosen but not yet confirmed by a snapshot
  };

  function rivalKey() {
    const s = state.snap;
    return combo.pending ?? (s.h2h.rival ? s.h2h.rival.key : "");
  }

  function rivalOptions() {
    const s = state.snap;
    const key = rivalKey();
    const row = s.standings.find((r) => r.key === key);
    const value = $("rival-value");
    value.replaceChildren(
      ...(row
        ? [el("span", { class: "select-rank", text: `#${row.pos}` }), el("span", { class: "select-name", text: row.name })]
        : [el("span", { class: "muted", text: `Pick a ${s.noun}` })]),
    );
    combo.search.placeholder = `Find a ${s.noun}`;
    if (!combo.pop.hidden) renderComboList(false);
  }

  function renderComboList(resetActive) {
    const s = state.snap;
    const q = combo.search.value.trim().toLowerCase().replace(/^#/, "");
    const key = rivalKey();
    const activeKey = combo.items[combo.active]?.key;
    combo.items = s.standings.filter(
      (r) => !r.is_me && (!q || r.name.toLowerCase().includes(q) || String(r.pos) === q),
    );
    replace(
      combo.list,
      combo.items.map((r, i) =>
        el(
          "li",
          { id: `rival-opt-${i}`, role: "option", class: "combo-item", "aria-selected": r.key === key ? "true" : "false", "data-i": i },
          el("span", { class: "select-rank", text: `#${r.pos}` }),
          el("span", { class: "select-name", text: r.name }),
          r.key === key ? icon("check") : null,
        ),
      ),
    );
    combo.none.hidden = combo.items.length > 0;
    let active = resetActive ? -1 : combo.items.findIndex((r) => r.key === activeKey);
    if (active < 0) active = q ? 0 : combo.items.findIndex((r) => r.key === key);
    setActive(active < 0 && combo.items.length ? 0 : active, true);
  }

  function setActive(i, scroll) {
    combo.list.querySelector(".combo-item[data-active]")?.removeAttribute("data-active");
    combo.active = i;
    const node = i >= 0 ? $(`rival-opt-${i}`) : null;
    if (node) {
      node.setAttribute("data-active", "");
      combo.search.setAttribute("aria-activedescendant", node.id);
      if (scroll) node.scrollIntoView({ block: "nearest" });
    } else {
      combo.search.removeAttribute("aria-activedescendant");
    }
  }

  function openCombo() {
    if (!state.snap || combo.trigger.disabled) return;
    combo.pop.hidden = false;
    combo.trigger.setAttribute("aria-expanded", "true");
    combo.search.value = "";
    renderComboList(true);
    combo.search.focus();
  }

  function closeCombo(focusTrigger) {
    if (combo.pop.hidden) return;
    combo.pop.hidden = true;
    combo.trigger.setAttribute("aria-expanded", "false");
    if (focusTrigger) combo.trigger.focus();
  }

  function chooseRival(i) {
    const r = combo.items[i];
    if (!r) return;
    closeCombo(true);
    if (r.key === rivalKey()) return;
    combo.pending = r.key;
    rivalOptions();
    api("rival", { key: r.key })
      .catch(() => {})
      .finally(() => {
        combo.pending = null;
      });
  }

  combo.trigger.addEventListener("click", () => (combo.pop.hidden ? openCombo() : closeCombo(false)));
  combo.trigger.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      openCombo();
    }
  });
  combo.search.addEventListener("input", () => renderComboList(true));
  combo.search.addEventListener("keydown", (e) => {
    const n = combo.items.length;
    const move = { ArrowDown: 1, ArrowUp: -1, PageDown: 8, PageUp: -8 }[e.key];
    if (move) {
      e.preventDefault();
      if (n) setActive(Math.min(n - 1, Math.max(0, combo.active + move)), true);
    } else if (e.key === "Enter") {
      e.preventDefault();
      chooseRival(combo.active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeCombo(true);
    } else if (e.key === "Tab") {
      closeCombo(false);
    }
  });
  combo.list.addEventListener("pointermove", (e) => {
    const item = e.target.closest(".combo-item");
    if (item && Number(item.dataset.i) !== combo.active) setActive(Number(item.dataset.i), false);
  });
  // Keep focus in the search field while clicking an option.
  combo.list.addEventListener("mousedown", (e) => e.preventDefault());
  combo.list.addEventListener("click", (e) => {
    const item = e.target.closest(".combo-item");
    if (item) chooseRival(Number(item.dataset.i));
  });
  document.addEventListener("pointerdown", (e) => {
    if (!$("rival-combo").contains(e.target)) closeCombo(false);
  });

  $("rival-reset").addEventListener("click", () => api("rival", { key: null }).catch(() => {}));
  function stepRival(delta) {
    const s = state.snap;
    const others = s.standings.filter((r) => !r.is_me);
    if (!others.length) return;
    const cur = s.h2h.rival ? others.findIndex((r) => r.key === s.h2h.rival.key) : -1;
    const next = others[Math.min(others.length - 1, Math.max(0, cur + delta))];
    if (next && next.key !== s.h2h.rival?.key) api("rival", { key: next.key }).catch(() => {});
  }
  $("rival-up").addEventListener("click", () => stepRival(-1));
  $("rival-down").addEventListener("click", () => stepRival(1));

  // One row per measure, you beside the rival, so each pair of numbers
  // sits in a column and compares at a glance.
  function versusRows(me, rival, rivalLoading) {
    const dash = "—";
    const pace = (p, k) => (p && p.momentum ? signed(p.momentum[k]) || "0" : dash);
    const last = (p) => (p && p.momentum && p.momentum.last_solve ? agoLong(p.momentum.last_solve) : dash);
    const solves = (p, loading) => (!p ? dash : loading ? "…" : p.solves === null ? dash : num(p.solves));
    const rows = [
      ["Rank", `#${me.pos}`, rival ? `#${rival.pos}` : dash],
      ["Score", num(me.score), rival ? num(rival.score) : dash],
      ["Solves", solves(me, false), solves(rival, rivalLoading)],
      ["Last hour", pace(me, "last_hour"), pace(rival, "last_hour")],
      ["Last 3 h", pace(me, "last_3h"), pace(rival, "last_3h")],
      ["Last solve", last(me), last(rival)],
    ];
    return rows.map(([label, mine, theirs]) =>
      el("tr", null, el("th", { scope: "row", text: label }), el("td", { class: "num", text: mine }), el("td", { class: "num", text: theirs })),
    );
  }

  // ------------------------------------------------------------ plan

  // Why a target stands out.  Keys come from the server; an unknown key
  // shows nothing rather than the key itself.
  const REASONS = {
    quick_win: { label: () => "Quick win", title: "Most of the field has solved it" },
    trending: { label: (t) => `${t.recent} solves this hour`, title: "Solved in the last hour by teams whose solves are tracked" },
    strength: { label: () => "Your category", title: "You solve more of this category than your average" },
    rival: { label: () => "Rival has it", title: "Solved by the team you compare with on Analysis" },
    unsolved: { label: () => "No solves yet", title: "Nobody has solved it" },
  };

  const pct = (p) => (p > 0 && p < 0.005 ? "<1%" : `${Math.round(p * 100)}%`);
  const solvesText = (n) => `${n} ${n === 1 ? "solve" : "solves"}`;

  // The rung the route aims at: the picked one while it is still ahead,
  // else the server's default (the qualifying line, or the team above).
  function goalRung() {
    const plan = state.snap.plan;
    const ahead = plan.ladder.filter((r) => r.ahead >= 0);
    return (
      ahead.find((r) => r.key === state.goal) ??
      ahead.find((r) => r.pos === plan.goal) ??
      ahead[ahead.length - 1] ??
      null
    );
  }

  const goalSelect = $("goal");
  goalSelect.addEventListener("change", () => {
    state.goal = goalSelect.value;
    renderPlan();
  });

  // Rebuilt only when the choices change, so an open menu is left alone.
  function renderGoalPicker(goal) {
    const s = state.snap;
    const ahead = s.plan.ladder.filter((r) => r.ahead >= 0);
    const label = (r) => (s.cutoff && r.pos === s.cutoff ? `#${r.pos} ${r.name} (top ${s.cutoff})` : `#${r.pos} ${r.name}`);
    const signature = ahead.map((r) => `${r.key}\n${label(r)}`).join("\n\n");
    if (goalSelect.dataset.signature !== signature) {
      goalSelect.dataset.signature = signature;
      replace(goalSelect, ahead.map((r) => el("option", { value: r.key, text: label(r) })));
    }
    goalSelect.closest(".row").hidden = ahead.length === 0;
    if (goal) goalSelect.value = goal.key;
  }

  function renderPlan() {
    const s = state.snap;
    const plan = s.plan;
    const me = s.h2h.me;
    $("plan-empty").hidden = !!me;
    $("plan-body").hidden = !me;
    if (!me) {
      $("plan-empty").textContent = `Your ${s.noun} isn't on the scoreboard yet`;
      return;
    }
    $("th-ladder-name").textContent = s.noun === "team" ? "Team" : "Player";

    const goal = goalRung();
    renderGoalPicker(goal);

    // Headline numbers.
    const leader = plan.ladder.find((r) => r.pos === 1 && r.ahead >= 0);
    const open = plan.targets.reduce((sum, t) => sum + t.value, 0);
    const best = plan.targets.length ? plan.targets[plan.targets.length - 1].reach : me.pos;
    const stat = (k, v, sub, cls) =>
      el("div", { class: "stat plan-stat" }, el("dt", { text: k }), el("dd", { class: cls }, v, sub ? el("small", { text: sub }) : null));
    const passIn = (r) => (r.solves ? solvesText(r.solves) : "Out of reach");
    const gain = (r) => (r.points ? `+${num(r.points)}` : null);
    const goalName = goal && (s.cutoff && goal.pos === s.cutoff ? `To reach top ${s.cutoff}` : `To pass #${goal.pos}`);
    replace($("plan-stats"), [
      stat("Rank", `#${me.pos}`, `${num(me.score)} pts`),
      leader ? stat("Behind #1", num(leader.ahead), null, "neg") : stat("Lead", num(-(plan.ladder.find((r) => r.ahead < 0)?.ahead ?? 0)), "pts", "pos"),
      goal && goal !== leader ? stat(goalName, passIn(goal), gain(goal)) : null,
      leader ? stat("To take #1", passIn(leader), gain(leader)) : null,
      stat("Open", num(open), `pts in ${plan.targets.length} ${plan.targets.length === 1 ? "challenge" : "challenges"}`),
      stat("Best reach", `#${best}`),
    ]);

    // The route to the goal first, in the order to try it, closed by a
    // line; then everything else, most likely points first.
    const byId = new Map(plan.targets.map((t) => [t.id, t]));
    const route = goal ? goal.route.map((id) => byId.get(id)).filter(Boolean) : [];
    const onRoute = new Set(route.map((t) => t.id));
    const targetRow = (t, step) => {
      const reason = REASONS[t.reason];
      const field = t.solves !== null ? `${num(t.solves)} of ${num(s.standings.length)} ${s.noun}s solved it` : `${t.top_solves} of the top ${s.top_n} solved it`;
      return el(
        "tr",
        { class: step ? "on-route" : null },
        el(
          "td",
          { class: "col-name" },
          el("span", { class: "chal" }, step ? el("span", { class: "step", text: step, title: `Step ${step} of the route` }) : null, el("span", { class: "chal-name", text: t.name, title: t.name })),
        ),
        el("td", { class: "hide-sm cat", text: t.category }),
        el("td", { class: "num", text: num(t.value) }),
        el(
          "td",
          { class: "num" },
          el("span", { class: "pop", title: field }, el("span", { text: pct(t.odds) }), el("span", { class: "pop-bar", "aria-hidden": "true" }, el("i", { style: `width:${Math.round(t.odds * 100)}%` }))),
        ),
        el("td", { class: "hide-sm col-why why", text: reason ? reason.label(t) : "", title: reason ? reason.title : null }),
      );
    };
    const targets = route.map((t, i) => targetRow(t, i + 1));
    if (route.length) {
      const label = s.cutoff && goal.pos === s.cutoff ? `Reaches top ${s.cutoff}, past ${goal.name}` : `Passes ${goal.name} for #${goal.pos}`;
      targets.push(el("tr", { class: "closes", "aria-hidden": "true" }, el("td", { colspan: 5 }, el("span", { text: label }))));
    }
    for (const t of plan.targets) if (!onRoute.has(t.id)) targets.push(targetRow(t, 0));
    replace($("targets-body"), targets);
    $("targets-empty").hidden = targets.length > 0;
    $("targets-empty").textContent = s.h2h.categories.length ? "Nothing left" : "Challenges aren't visible yet";

    const note = $("route-note");
    if (!goal) note.textContent = plan.targets.length ? "You lead the board" : "";
    else if (!route.length) note.textContent = `Not enough open points to pass #${goal.pos}`;
    else note.textContent = `${solvesText(route.length)} for +${num(goal.points)}, ${pct(goal.odds)} chance of landing ${route.length === 1 ? "it" : "all"}`;

    // Ladder, with you in your place.
    const rows = [];
    const youRow = () =>
      el(
        "tr",
        { class: "is-me" },
        el("td", { class: "num col-rank", text: me.pos }),
        el("td", { class: "col-name" }, el("div", { class: "team" }, el("span", { class: "team-name", text: me.name, title: me.name }), el("span", { class: "badge badge-solid badge-sm", text: "You" }))),
        el("td"),
        el("td", { class: "hide-sm" }),
        el("td"),
        el("td", { class: "hide-sm" }),
      );
    let placed = false;
    for (const r of plan.ladder) {
      if (!placed && r.pos > me.pos) {
        rows.push(youRow());
        placed = true;
      }
      const gap = -r.ahead;
      const m = r.momentum;
      const isGoal = r === goal;
      rows.push(
        el(
          "tr",
          { class: isGoal ? "is-goal" : null },
          el("td", { class: "num col-rank", text: r.pos }),
          el("td", { class: "col-name" }, el("div", { class: "team" }, el("span", { class: "team-name", text: r.name, title: r.name }), isGoal ? el("span", { class: "badge badge-outline badge-sm", text: "Goal" }) : null)),
          el("td", { class: `num ${gap > 0 ? "delta-up" : gap < 0 ? "delta-down" : "muted"}`, text: signed(gap) || "0" }),
          el("td", { class: "num hide-sm", text: m ? signed(m.last_hour) || "0" : "—" }),
          el("td", { class: "num", title: r.points ? `+${num(r.points)}` : null, text: r.ahead < 0 ? "" : r.solves ? solvesText(r.solves) : "—" }),
          el("td", { class: "num hide-sm muted", text: r.ahead < 0 || r.odds === null ? "" : pct(r.odds) }),
        ),
      );
    }
    if (!placed) rows.push(youRow());
    replace($("ladder-body"), rows);

    replace(
      $("focus-body"),
      plan.focus.map((f) =>
        el(
          "tr",
          null,
          el("td", { class: "cat-name", text: f.category || "Uncategorized" }),
          el("td", { class: "num muted", text: `${f.mine}/${f.total}` }),
          el("td", { class: "num", text: num(f.open) }),
          el("td", { class: "num hide-sm", text: num(f.points) }),
          el("td", { class: "num", text: num(Math.round(f.expected)) }),
        ),
      ),
    );
    $("focus-empty").hidden = plan.focus.length > 0;
  }

  function challengeCells(c) {
    return [
      el("td", { class: "col-name" }, el("span", { class: "chal-name", text: c.name, title: c.name })),
      el("td", { class: "hide-sm col-cat cat", text: c.category }),
      el("td", { class: "num", text: num(c.value) }),
    ];
  }

  function popularity(c) {
    const s = state.snap;
    const pct = Math.round((c.top_solves / Math.max(1, s.top_n)) * 100);
    return el(
      "td",
      { class: "num" },
      el("span", { class: "pop", title: `${c.top_solves} of the top ${s.top_n} solved it` }, el("span", { text: `${c.top_solves}/${s.top_n}` }), el("span", { class: "pop-bar", "aria-hidden": "true" }, el("i", { style: `width:${pct}%` }))),
    );
  }

  function renderAnalysis() {
    const s = state.snap;
    const h = s.h2h;
    rivalOptions();
    const hasMe = !!h.me;
    $("analysis-empty").hidden = hasMe;
    $("analysis-body").hidden = !hasMe;
    if (!hasMe) {
      $("analysis-empty").textContent = `Your ${s.noun} isn't on the scoreboard yet`;
      return;
    }

    replace($("h2h-rows"), versusRows(h.me, h.rival, h.rival_loading));

    // The verdict: how far apart, then where the qualifying line is.
    const gap = [];
    if (h.rival) {
      const d = h.me.score - h.rival.score;
      gap.push(
        el(
          "p",
          { class: "versus-verdict" },
          el("span", { class: `versus-num${d > 0 ? " ahead" : d < 0 ? " behind" : ""}`, text: d === 0 ? "Tied" : num(Math.abs(d)) }),
          el("span", { class: "versus-word", text: d === 0 ? "on points" : d > 0 ? "ahead" : "behind" }),
        ),
      );
    } else {
      gap.push(el("p", { class: "versus-verdict" }, el("span", { class: "versus-word", text: "No rival" })));
    }
    if (h.cutoff) {
      const c = h.cutoff;
      const text = c.inside
        ? c.points > 0
          ? `Qualifying by ${num(c.points)}`
          : "Qualifying on tiebreak"
        : `${num(Math.abs(c.points))} short of top ${s.cutoff}`;
      gap.push(el("p", { class: "versus-line", text }));
    }
    replace($("h2h-gap"), gap);

    // Catch-up targets.
    const catchBody = [];
    const catchEmpty = $("catch-empty");
    if (!h.rival) {
      catchEmpty.textContent = "No rival";
    } else if (h.rival_loading) {
      catchEmpty.textContent = "Loading…";
    } else {
      catchEmpty.textContent = "Nothing to catch up on";
      let running = 0;
      h.catch_up.forEach((c, i) => {
        running += c.value;
        catchBody.push(el("tr", null, ...challengeCells(c), popularity(c), el("td", { class: "num hide-sm col-extra muted", text: num(running) })));
        if (i === h.closes_gap_at && i < h.catch_up.length - 1) {
          const gapPts = h.rival.score - h.me.score;
          catchBody.push(el("tr", { class: "closes", "aria-hidden": "true" }, el("td", { colspan: 5 }, el("span", { text: `Passes ${h.rival.name}`, title: `Solving down to here closes the ${num(gapPts)} point gap` }))));
        }
      });
    }
    replace($("catch-body"), catchBody);
    catchEmpty.hidden = catchBody.length > 0;

    // Edge.
    const edgeRows = h.rival && !h.rival_loading ? h.edge.map((c) => el("tr", null, ...challengeCells(c))) : [];
    replace($("edge-body"), edgeRows);
    $("edge-empty").hidden = edgeRows.length > 0;
    $("edge-empty").textContent = !h.rival ? "No rival" : h.rival_loading ? "Loading…" : "Nothing yet";

    // Field.
    const fieldRows = h.field.map((c) => el("tr", null, ...challengeCells(c), popularity(c), el("td", { class: "num hide-sm col-extra muted", text: c.solves === null ? "—" : num(c.solves) })));
    replace($("field-body"), fieldRows);
    $("field-empty").hidden = fieldRows.length > 0;

    renderCategoryChart();
  }

  // ------------------------------------------------------------ render

  function render() {
    const s = state.snap;
    renderHeader();
    renderStatus();
    if (state.view === "board") {
      assignSlots(s.lines);
      renderProgress();
      renderStandings();
      renderFeed();
    } else if (state.view === "analysis") {
      assignSlots(s.lines);
      renderAnalysis();
    } else {
      assignSlots(s.lines);
      renderPlan();
    }
    if (!s.refreshing) refreshBtn.removeAttribute("aria-busy");
  }

  // Relative times tick without waiting for the next snapshot.
  setInterval(() => {
    if (!state.snap) return;
    renderStatus();
    for (const node of document.querySelectorAll(".feed-time[data-t]")) node.textContent = ago(Number(node.dataset.t));
  }, 1000);

  // ------------------------------------------------------------ connect

  function accept(json) {
    const snap = JSON.parse(json);
    state.offset = snap.now - Date.now() / 1000;
    state.snap = snap;
    render();
  }

  function connect() {
    if (!KEY) {
      showFatal("This page needs the link ctfdc printed", "Run ctfdc lb and open the link it shows. The key at the end of it is required.");
      return;
    }
    const source = new EventSource(`api/events?k=${encodeURIComponent(KEY)}`);
    source.addEventListener("board", (e) => {
      state.connected = true;
      state.everConnected = true;
      accept(e.data);
    });
    source.addEventListener("open", () => {
      state.connected = true;
      state.everConnected = true;
      if (state.snap) renderStatus();
    });
    source.addEventListener("error", async () => {
      state.connected = false;
      if (state.snap) renderStatus();
      if (!state.everConnected) {
        // Tell a wrong key apart from a stopped server.
        try {
          const res = await fetch("api/state", { headers: { "x-ctfdc-key": KEY } });
          if (res.status === 401) {
            source.close();
            showFatal("This link has expired", "ctfdc makes a new link each time it starts. Open the link it printed most recently.");
          }
        } catch {
          showFatal("Can't reach ctfdc", "Start it again with ctfdc lb.");
        }
      }
    });
  }

  function showFatal(title, body) {
    $("alert").hidden = false;
    $("alert-title").textContent = title;
    $("alert-body").textContent = body;
    $("status").dataset.state = "offline";
    $("status").textContent = "Not connected";
    $("host").textContent = "Not connected";
    $("view-board").hidden = true;
    $("view-analysis").hidden = true;
    $("view-plan").hidden = true;
  }

  showView(state.view);
  connect();
})();
