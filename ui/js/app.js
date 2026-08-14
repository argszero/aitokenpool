/* ============================================================
   AITokenPool UI Prototype — 交互逻辑（纯原生 JS）
   无框架 / 无构建 / 无外部依赖
   ============================================================ */

(function () {
  "use strict";

  const D = window.ATDATA;
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => Array.from(document.querySelectorAll(sel));
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  let activeView = "dashboard";
  let txTab = "all";

  // MRT 风格表格状态（页面级变量：切换页面不丢失排序/筛选/分页）
  const txTable = { sort: [], filters: {}, page: 1, pageSize: 10 };

  /* ---------------- 工具 ---------------- */

  function toast(msg) {
    const el = $("#toast");
    el.textContent = msg;
    el.classList.remove("hidden");
    clearTimeout(toast._t);
    toast._t = setTimeout(() => el.classList.add("hidden"), 2400);
  }

  function badge(status, labels) {
    return '<span class="badge ' + (labels[status] ? labels[status].cls : "dim") + '">' +
      esc(labels[status] ? labels[status].text : status) + "</span>";
  }

  // 自动单价：单价是模型×厂商的客观属性，由平台按模型定价自动计算（输出 1M tokens 折算点数）
  function autoPrice(model) {
    const m = D.MODELS.find((x) => x.model === model);
    if (m && typeof m.out === "number") return m.out;
    // 兜底：取同厂商相近模型的输出价；仍无则用固定默认价
    const same = m ? D.MODELS.find((x) => x.provider === m.provider && typeof x.out === "number") : null;
    return same ? same.out : 300;
  }

  // key 脱敏展示：仅显示前 3 后 4（如 sk-****1234）
  function maskKey(key) {
    if (!key) return "—";
    if (key.length <= 8) return key.slice(0, 3) + "****" + key.slice(-4);
    return key.slice(0, 3) + "****" + key.slice(-4);
  }

  function showPriceHint(model) {
    const el = $("#sf-price-view");
    if (!el) return;
    const m = D.MODELS.find((x) => x.model === model);
    if (!model) { el.textContent = "选择模型后自动计算"; return; }
    if (m && typeof m.out === "number") {
      el.textContent = D.fmt(m.out) + " 点 / 1M 输出（自动）";
    } else {
      el.textContent = "按默认价：" + D.fmt(autoPrice(model)) + " 点 / 1M 输出（自动）";
    }
  }

  /* ---------------- 导航 ---------------- */

  const NAV = [
    { g: "主导航", items: [
      { id: "dashboard", ico: "📊", label: "仪表盘 Dashboard" },
      { id: "marketplace", ico: "🛒", label: "模型市场 Marketplace" },
      { id: "sharing", ico: "🔗", label: "共享管理 Sharing" },
      { id: "wallet", ico: "👛", label: "钱包 Wallet" },
      { id: "transactions", ico: "🧾", label: "交易记录 Transactions" },
    ]},
    { g: "角色视图", items: [
      { id: "admin", ico: "🛠️", label: "管理视图 Admin", role: "admin" },
      { id: "settings", ico: "⚙️", label: "设置 Settings" },
    ]},
  ];

  const VIEW_TITLE = {
    dashboard: "仪表盘 Dashboard", marketplace: "模型市场 Marketplace", sharing: "共享管理 Sharing",
    wallet: "钱包 Wallet", transactions: "交易记录 Transactions", settings: "设置 Settings",
    admin: "管理视图 Admin",
  };

  function renderNav() {
    const nav = $("#nav");
    nav.innerHTML = "";
    NAV.forEach((group) => {
      const g = document.createElement("div");
      g.className = "nav-group";
      g.textContent = group.g;
      nav.appendChild(g);
      group.items.forEach((item) => {
        const b = document.createElement("button");
        b.className = "nav-item" + (item.id === activeView ? " active" : "");
        b.dataset.view = item.id;
        b.innerHTML = '<span class="ico">' + item.ico + '</span><span class="label">' + esc(item.label) + "</span>";
        if (item.role) {
          const tag = document.createElement("span");
          tag.className = "nav-tag";
          tag.textContent = "管理员";
          b.appendChild(tag);
        }
        b.addEventListener("click", () => switchView(item.id));
        nav.appendChild(b);
      });
    });
    $("#mode-label").textContent = "共享市场 · 角色视图";
  }

  function switchView(id) {
    activeView = id;
    $$(".view").forEach((v) => v.classList.add("hidden"));
    $("#view-" + id).classList.remove("hidden");
    renderNav();
    renderView(id);
    $("#main").scrollTop = 0;
  }

  /* ---------------- 视图渲染 ---------------- */

  function renderView(id) {
    if (id === "dashboard") renderDashboard();
    else if (id === "marketplace") renderMarketplace();
    else if (id === "sharing") renderSharing();
    else if (id === "wallet") renderWallet();
    else if (id === "transactions") renderTransactions();
    else if (id === "settings") renderSettings();
    else if (id === "admin") renderAdmin();
  }

  /* --- 仪表盘 --- */

  function renderDashboard() {
    const txs = D.TRANSACTIONS;
    const monthUse = txs.filter((t) => t.type === "consume").reduce((a, t) => a + Math.abs(t.pts), 0);
    const monthEarn = txs.filter((t) => t.type === "earn").reduce((a, t) => a + t.pts, 0);
    const recent = txs.slice(0, 5);

    $("#dash-stats").innerHTML = [
      stat("点数余额 Points", D.fmt(D.USER.balance), "1 点 ≈ 1 元", "accent"),
      stat("本月用量 Usage", D.fmt(monthUse) + " 点", "共 " + txs.filter((t) => t.type === "consume").length + " 笔消费"),
      stat("共享收益 Earnings", "+" + D.fmt(monthEarn) + " 点", D.SHARINGS.filter((s) => s.status === "on").length + " 个 key 上架中"),
      stat("交易笔数 Trades", txs.length + " 笔", "含充值 / 提现 / 消费 / 收益 / 赠送"),
    ].join("");

    $("#dash-recent").innerHTML = recent.map((t) =>
      '<div class="mini-item"><div><div class="t">' + esc(t.detail) + "</div>" +
      '<div class="d">' + esc(t.time) + " · " + esc(t.partner) + "</div></div>" +
      '<div class="r"><span class="pts">' + (t.pts > 0 ? "+" : "") + D.fmt(t.pts) + "</span></div></div>"
    ).join("") + (recent.length ? "" : '<p class="muted">暂无交易</p>');

    const on = D.SHARINGS.filter((s) => s.status === "on");
    $("#dash-sharings").innerHTML = on.map((s) =>
      '<div class="mini-item"><div><div class="t">' + esc(s.model) + "</div>" +
      '<div class="d">已用 ' + D.fmt(s.used) + " / " + D.fmt(s.quota) + " 点 · 单价 " + D.fmt(s.price) + " 点/1M</div></div>" +
      '<div class="r"><span class="pts">+' + D.fmt(s.earned) + "</span><div class='d'>累计收益</div></div></div>"
    ).join("") + (on.length ? "" : '<p class="muted">还没有上架的 key</p>');
  }

  function stat(label, value, sub, cls) {
    return '<div class="stat' + (cls ? " " + cls : "") + '"><div class="label">' + esc(label) +
      '</div><div class="value">' + value + "</div><div class='sub'>" + esc(sub) + "</div></div>";
  }

  /* --- 模型市场 --- */

  function renderMarketplace() {
    const q = ($("#mk-search").value || "").toLowerCase();
    const prov = $("#mk-provider").value;
    const sort = $("#mk-sort").value;

    let list = D.MARKET.filter((m) =>
      (!q || m.model.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q)) &&
      (!prov || m.provider === prov)
    );
    if (sort === "price-asc") list = [...list].sort((a, b) => a.in - b.in);
    else if (sort === "price-desc") list = [...list].sort((a, b) => b.in - a.in);
    else if (sort === "ctx-desc") list = [...list].sort((a, b) => b.ctx - a.ctx);

    $("#mk-count").textContent = list.length + " 个在售 key";
    $("#mk-body").innerHTML = list.length ? list.map((m) =>
      "<tr><td>" + esc(m.provider) + "</td><td><strong>" + esc(m.model) + "</strong></td>" +
      "<td>" + D.fmt(m.in) + " 点</td><td>" + D.fmt(m.out) + " 点</td>" +
      "<td>" + D.ctxFmt(m.ctx) + "</td>" +
      "<td>" + (m.avail ? '<span class="badge ok">可用</span>' : '<span class="badge warn">繁忙</span>') + "</td>" +
      "<td><span class='muted'>成功率 " + m.success + "%</span></td></tr>"
    ).join("") : '<tr><td colspan="7" class="muted">没有匹配的模型</td></tr>';
  }

  /* --- 共享管理 --- */

  const SHARE_STATUS = {
    on: { text: "上架中", cls: "ok" },
    paused: { text: "已暂停", cls: "warn" },
    off: { text: "已下线", cls: "dim" },
  };

  function renderSharing() {
    const on = D.SHARINGS.filter((s) => s.status === "on");
    const totalEarned = D.SHARINGS.reduce((a, s) => a + s.earned, 0);
    const totalUsed = D.SHARINGS.reduce((a, s) => a + s.used, 0);

    $("#share-stats").innerHTML = [
      stat("上架中 Listings", on.length + " 个", D.SHARINGS.length + " 个历史"),
      stat("累计收益 Earnings", "+" + D.fmt(totalEarned) + " 点", "全部 key"),
      stat("已用量 Used", D.fmt(totalUsed) + " 点", D.SHARINGS.reduce((a, s) => a + s.quota, 0) + " 点总额度"),
    ].join("");

    // 表单下拉
    const selP = $("#sf-provider");
    if (!selP.dataset.init) {
      selP.innerHTML = '<option value="">选择厂商</option>' + D.PROVIDERS.map((p) => '<option value="' + p + '">' + p + "</option>").join("");
      const selM = $("#sf-model");
      const fillModels = () => {
        const p = selP.value;
        selM.innerHTML = '<option value="">选择模型</option>' + D.MODELS.filter((m) => !p || m.provider === p)
          .map((m) => '<option value="' + m.model + '">' + m.model + "</option>").join("");
        showPriceHint(selM.value);
      };
      selP.addEventListener("change", fillModels);
      selM.addEventListener("change", () => showPriceHint(selM.value));
      selP.dataset.init = "1";
      fillModels();
    }

    $("#share-body").innerHTML = D.SHARINGS.map((s, i) =>
      "<tr><td><strong>" + esc(s.model) + "</strong></td>" +
      "<td class='mono'>" + esc(maskKey(s.key)) + "</td>" +
      "<td>" + D.fmt(s.used) + " / " + D.fmt(s.quota) + "</td>" +
      "<td>" + D.fmt(s.price) + " 点/1M</td>" +
      "<td>+" + D.fmt(s.earned) + " 点</td>" +
      "<td>" + badge(s.status, SHARE_STATUS) + "</td>" +
      "<td><button class='btn btn-ghost' data-share-toggle='" + i + "' style='padding:4px 10px;font-size:12px'>" +
      (s.status === "on" ? "暂停" : s.status === "paused" ? "恢复" : "重新上架") + "</button> " +
      "<button class='btn btn-danger' data-share-delete='" + i + "' style='padding:4px 10px;font-size:12px'>删除</button></td></tr>"
    ).join("");
  }

  function deleteSharing(i) {
    const s = D.SHARINGS[i];
    if (!s) return;
    if (!window.confirm("确认彻底下架 " + s.model + " 的 key？删除后不再共享，不可恢复。")) return;
    D.SHARINGS.splice(i, 1);
    renderSharing();
    if (activeView === "dashboard") renderDashboard();
    toast("已删除 " + s.model + " 的 key（彻底下架）");
  }

  function toggleSharing(i) {
    const s = D.SHARINGS[i];
    if (s.status === "on") { s.status = "paused"; toast("已暂停 " + s.model + " 的共享"); }
    else if (s.status === "paused") { s.status = "on"; toast("已恢复 " + s.model + " 的共享"); }
    else { s.status = "on"; s.quota = 50000; s.used = 0; toast("已重新上架 " + s.model); }
    renderSharing();
    if (activeView === "dashboard") renderDashboard();
  }

  /* --- 钱包 --- */

  function renderWallet() {
    // 钱包只做余额与资金操作；收支明细统一到【交易记录】（见 index.html wallet-hint）
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    $("#wallet-balance").textContent = D.fmt(D.USER.balance);
    // 点数来源 / 有效期（v1.4：每日赠送 1 点当日有效、收益/充值永久）
    const src = $("#points-sources");
    if (src) {
      src.innerHTML = D.POINT_SOURCES.map((s) =>
        '<div class="mini-item"><div><div class="t">' + esc(s.label) + "</div>" +
        '<div class="d">' + esc(s.note) + "</div></div>" +
        '<div class="r"><span class="pts">' + (s.pts > 0 ? "+" : "") + D.fmt(s.pts) + "</span></div></div>"
      ).join("");
    }
  }

  /* --- 交易记录 --- */

  const TX_TYPE = {
    consume: "消费", earn: "收益", topup: "充值", withdraw: "提现", gift: "赠送",
  };

  const TX_COLUMNS = [
    { key: "time", title: "时间", sort: "string", filter: "text" },
    { key: "type", title: "类型", sort: "string", filter: "select", options: ["消费", "收益", "充值", "提现", "赠送"], filterVal: (t) => TX_TYPE[t.type] || t.type,
      render: (t) => t.type === "earn" ? '<span class="badge ok">收益</span>' : t.type === "consume" ? '<span class="badge accent">消费</span>' : t.type === "gift" ? '<span class="badge ok">赠送</span>' : '<span class="badge dim">' + esc(TX_TYPE[t.type] || t.type) + "</span>" },
    { key: "partner", title: "模型 / Key", sort: "string", filter: "text" },
    { key: "tokens", title: "Token 用量", sort: "string", filter: "text" },
    { key: "pts", title: "点数", sort: "number", filter: "number-range",
      render: (t) => '<span style="color:' + (t.pts > 0 ? "var(--ok)" : "var(--text)") + ';font-weight:600">' + (t.pts > 0 ? "+" : "") + D.fmt(t.pts) + "</span>" },
    { key: "status", title: "状态", sort: "string", filter: "select", options: ["成功", "入账", "处理中"],
      render: (t) => t.status === "处理中" ? '<span class="badge warn">' + esc(t.status) + "</span>" : esc(t.status) },
  ];

  function renderTransactions() {
    $$("#tx-tabs .tab").forEach((b) => b.classList.toggle("active", b.dataset.txTab === txTab));
    let list = D.TRANSACTIONS;
    if (txTab === "consume") list = list.filter((t) => t.type === "consume");
    else if (txTab === "earn") list = list.filter((t) => t.type === "earn");
    buildDataTable({
      container: $("#tx-table"),
      columns: TX_COLUMNS,
      rows: list,
      state: txTable,
      onState: renderTransactions,
    });
  }

  /* --- 通用 MRT 风格数据表格渲染器 ---
     cfg: { container, columns, rows, state, onState }
     columns: [{ key, title, sort?: "string"|"number", filter?: "text"|"select"|"number-range", options?, render? }]
     state:  { sort: [{key,dir}], filters: {key:val}, page, pageSize }（原地更新，跨页保留） */
  function buildDataTable(cfg) {
    const { container, columns, rows, state, onState } = cfg;

    // 1) 筛选
    let data = rows.filter((row) => {
      for (const key of Object.keys(state.filters)) {
        const fv = state.filters[key];
        if (fv == null || fv === "") continue;
        const col = columns.find((c) => c.key === key);
        if (!col || !col.filter) continue;
        const v = col.filterVal ? col.filterVal(row) : row[key];
        if (col.filter === "select") { if (String(v) !== String(fv)) return false; }
        else if (col.filter === "number-range") {
          const parts = String(fv).split(":");
          const min = parts[0] === "" ? NaN : Number(parts[0]);
          const max = parts[1] === "" || parts[1] == null ? NaN : Number(parts[1]);
          if (!isNaN(min) && Number(v) < min) return false;
          if (!isNaN(max) && Number(v) > max) return false;
        } else {
          if (!String(v).toLowerCase().includes(String(fv).toLowerCase())) return false;
        }
      }
      return true;
    });

    // 2) 排序（多列：Shift 点击叠加）
    if (state.sort.length) {
      data = data.slice().sort((a, b) => {
        for (const sk of state.sort) {
          const col = columns.find((c) => c.key === sk.key);
          const av = a[sk.key], bv = b[sk.key];
          let cmp;
          if (col && col.sort === "number") cmp = Number(av) - Number(bv);
          else cmp = String(av).localeCompare(String(bv), "zh-CN");
          if (cmp !== 0) return sk.dir === "asc" ? cmp : -cmp;
        }
        return 0;
      });
    }

    // 3) 分页
    const pages = Math.max(1, Math.ceil(data.length / state.pageSize));
    if (state.page > pages) state.page = pages;
    const pageRows = data.slice((state.page - 1) * state.pageSize, state.page * state.pageSize);

    // 4) 渲染表头（排序按钮）+ 筛选行
    let html = '<table class="table"><thead><tr>';
    columns.forEach((col) => {
      const sk = state.sort.find((s) => s.key === col.key);
      const arrow = sk ? (sk.dir === "asc" ? " ▲" : " ▼") : "";
      html += '<th><button type="button" class="th-sort" data-sort-key="' + esc(col.key) + '" title="点击排序 · Shift+点击叠加多列">' +
        esc(col.title) + arrow + "</button></th>";
    });
    html += "</tr><tr>";
    columns.forEach((col) => {
      const fv = state.filters[col.key] != null ? String(state.filters[col.key]) : "";
      if (col.filter === "select") {
        const opts = (col.options || []).map((o) =>
          '<option value="' + esc(o) + '"' + (fv === String(o) ? " selected" : "") + ">" + esc(o) + "</option>").join("");
        html += '<td><select class="th-filter" data-filter-key="' + esc(col.key) + '"><option value="">全部</option>' + opts + "</select></td>";
      } else if (col.filter === "number-range") {
        const p = fv ? fv.split(":") : ["", ""];
        html += '<td class="range-filter"><input class="th-filter" data-filter-key="' + esc(col.key) + '" data-range="min" placeholder="最小" value="' + esc(p[0] || "") + '">' +
          '<input class="th-filter" data-filter-key="' + esc(col.key) + '" data-range="max" placeholder="最大" value="' + esc(p[1] || "") + '"></td>';
      } else if (col.filter) {
        html += '<td><input class="th-filter" data-filter-key="' + esc(col.key) + '" placeholder="筛选…" value="' + esc(fv) + '"></td>';
      } else {
        html += "<td></td>";
      }
    });
    html += "</tr></thead><tbody>";
    if (!pageRows.length) html += '<tr><td colspan="' + columns.length + '" class="muted">没有匹配的记录</td></tr>';
    pageRows.forEach((row) => {
      html += "<tr>";
      columns.forEach((col) => {
        html += "<td>" + (col.render ? col.render(row) : esc(row[col.key] == null ? "" : row[col.key])) + "</td>";
      });
      html += "</tr>";
    });
    html += "</tbody></table>";

    // 5) 分页器 + 每页行数
    if (pages > 1) {
      html += '<div class="pager">';
      for (let i = 1; i <= pages; i++) html += '<button type="button" class="' + (i === state.page ? "active" : "") + '" data-p="' + i + '">' + i + "</button>";
      html += "<span>" + state.page + " / " + pages + " · 共 " + data.length + " 条</span></div>";
    }
    html += '<div class="pager-size">每页 <select data-page-size><option value="5">5</option><option value="10">10</option><option value="25">25</option><option value="50">50</option></select> 条</div>';

    container.innerHTML = html;

    // 6) 事件绑定
    container.querySelectorAll("[data-sort-key]").forEach((b) => {
      b.addEventListener("click", (e) => {
        const key = b.dataset.sortKey;
        const ex = state.sort.find((s) => s.key === key);
        if (ex) {
          if (ex.dir === "asc") ex.dir = "desc";
          else state.sort = state.sort.filter((s) => s.key !== key);
        } else {
          if (!e.shiftKey) state.sort = [];
          state.sort.push({ key, dir: "asc" });
        }
        state.page = 1;
        onState();
      });
    });
    container.querySelectorAll(".th-filter").forEach((el) => {
      el.addEventListener("input", () => {
        const key = el.dataset.filterKey;
        const range = el.dataset.range;
        if (range) {
          const other = container.querySelector('[data-filter-key="' + key + '"][data-range="' + (range === "min" ? "max" : "min") + '"]');
          const min = range === "min" ? el.value : (other ? other.value : "");
          const max = range === "max" ? el.value : (other ? other.value : "");
          state.filters[key] = min + ":" + max;
        } else {
          state.filters[key] = el.value;
        }
        state.page = 1;
        const focusSel = range ? '[data-filter-key="' + key + '"][data-range="' + range + '"]' : '[data-filter-key="' + key + '"]';
        onState();
        const n = container.querySelector(focusSel);
        if (n) n.focus();
      });
    });
    container.querySelectorAll("[data-p]").forEach((b) => {
      b.addEventListener("click", () => { state.page = Number(b.dataset.p); onState(); });
    });
    const ps = container.querySelector("[data-page-size]");
    if (ps) {
      ps.value = state.pageSize;
      ps.addEventListener("change", () => { state.pageSize = Number(ps.value); state.page = 1; onState(); });
    }
  }

  /* --- 设置 --- */

  // API Key 脱敏展示：atk_live_****xxxx（复制时给完整 id）
  function maskAtk(id) {
    if (!id) return "—";
    if (id.startsWith("atk_live_")) return "atk_live_****" + id.slice(-4);
    return maskKey(id);
  }

  function renderSettings() {
    const q = ($("#ak-search").value || "").toLowerCase();
    const list = D.API_KEYS.filter((k) => !q || k.name.toLowerCase().includes(q));
    $("#api-keys").innerHTML = list.length ? list.map((k, i) =>
      "<tr><td><strong>" + esc(k.name) + "</strong></td>" +
      "<td><code>" + esc(maskAtk(k.id)) + "</code></td>" +
      "<td>" + esc(k.created) + "</td>" +
      "<td>" + esc(k.last) + "</td>" +
      "<td>" + (k.status === "active" ? '<span class="badge ok">启用</span>' : '<span class="badge dim">' + esc(k.status || "—") + "</span>") + "</td>" +
      "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-copy='" + i + "'>复制</button> " +
      "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-rename='" + i + "'>改名</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-key-del='" + i + "'>删除</button></td></tr>"
    ).join("") : '<tr><td colspan="6" class="muted">没有匹配的 key</td></tr>';
  }

  // 一键复制完整 key；file:// 下 clipboard API 受限 → 降级：临时 textarea 选中 + execCommand("copy")，仍失败则提示 Ctrl+C
  function copyKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    const okToast = () => toast("已复制「" + k.name + "」完整 key 到剪贴板");
    const fallback = () => {
      const ta = document.createElement("textarea");
      ta.value = k.id;
      ta.style.cssText = "position:fixed;opacity:0";
      document.body.appendChild(ta);
      ta.select();
      let ok = false;
      try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
      document.body.removeChild(ta);
      if (ok) okToast();
      else toast("已选中完整 key，请按 Ctrl+C / Cmd+C 复制");
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(k.id).then(okToast).catch(fallback);
    } else {
      fallback();
    }
  }

  function renameKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    const input = window.prompt("修改 key 名字：", k.name);
    if (input == null) return;
    const name = String(input).trim();
    if (!name) { toast("名字不能为空（未生效）"); return; }
    k.name = name;
    renderSettings();
    toast("已改名为「" + name + "」");
  }

  function deleteKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    if (!window.confirm("删除后该 key 立即失效，确认删除「" + k.name + "」？")) return;
    D.API_KEYS.splice(i, 1);
    renderSettings();
    toast("已删除 key「" + k.name + "」");
  }

  /* --- 管理员角色视图 --- */

  const KEY_STATUS = {
    ok: { text: "可用", cls: "ok" },
    limit: { text: "达限额", cls: "warn" },
    exhausted: { text: "已用尽", cls: "danger" },
    revoked: { text: "已撤销", cls: "dim" },
  };

  function renderAdmin() {
    const tab = $("#admin-tabs .tab.active").dataset.adminTab;
    $$(".admin-pane").forEach((p) => p.classList.toggle("hidden", p.dataset.adminPane !== tab));

    if (tab === "keys") {
      const q = ($("#ak-search").value || "").toLowerCase();
      const rows = D.KEYS.filter((k) => !q || k.model.includes(q) || k.provider.includes(q) || k.key.includes(q));
      $("#keys-body").innerHTML = rows.map((k) =>
        "<tr><td>" + esc(k.provider) + "</td><td><strong>" + esc(k.model) + "</strong></td>" +
        "<td><code>" + esc(k.key) + "</code></td>" +
        "<td>" + D.fmt(k.used) + " / " + D.fmt(k.quota) + "</td>" +
        "<td>" + badge(k.status, KEY_STATUS) + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-revoke='" + k.id + "'>" +
        (k.status === "revoked" ? "恢复" : "撤销") + "</button></td></tr>"
      ).join("");
    } else if (tab === "employees") {
      const total = D.EMPLOYEES.reduce((a, e) => a + e.quota, 0);
      const used = D.EMPLOYEES.reduce((a, e) => a + e.used, 0);
      const unassigned = D.EMPLOYEES.filter((e) => !e.dept).length;
      $("#emp-stats").innerHTML = [
        stat("成员数 Members", D.EMPLOYEES.length + " 人", D.DEPARTMENTS.length + " 个部门" + (unassigned ? " · 未分配 " + unassigned + " 人" : "")),
        stat("总配额 Quota", D.fmt(total) + " 点", "月"),
        stat("已用 Usage", D.fmt(used) + " 点", Math.round((used / total) * 100) + "% 消耗率"),
        stat("剩余 Remain", D.fmt(total - used) + " 点", "按成员分配"),
      ].join("");
      $("#emp-body").innerHTML = D.EMPLOYEES.map((e, i) =>
        "<tr data-emp-row='" + i + "'><td><strong>" + esc(e.name) + "</strong></td>" +
        "<td>" + (e.dept ? esc(e.dept) : '<span class="muted">未分配</span>') + "</td>" +
        "<td>" + D.fmt(e.used) + " / " + D.fmt(e.quota) + "</td>" +
        "<td>" + D.fmt(e.quota - e.used) + "</td>" +
        "<td>" + (e.used / e.quota > 0.9 ? '<span class="badge warn">接近限额</span>' : '<span class="badge ok">正常</span>') + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-dept='" + i + "'>改部门</button> " +
        "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-topup='" + i + "'>充值</button></td></tr>"
      ).join("");
    } else if (tab === "usage") {
      const maxM = Math.max(...D.USAGE_MODEL.map((u) => u.pts));
      const maxE = Math.max(...D.USAGE_EMP.map((u) => u.pts));
      $("#usage-model").innerHTML = D.USAGE_MODEL.map((u) => barRow(u.name, u.pts, maxM, "点")).join("");
      $("#usage-emp").innerHTML = D.USAGE_EMP.map((u) => barRow(u.name, u.pts, maxE, "点")).join("");
    } else if (tab === "org") {
      renderOrg();
    }
  }

  /* --- 组织管理：部门列表 + 部门 CRUD + 每月点数分配 --- */

  // 成员改部门：行内下拉（选项来自 DEPARTMENTS + "未分配"），确认后更新并联动部门统计
  function editEmpDept(i) {
    const emp = D.EMPLOYEES[i];
    const row = document.querySelector('[data-emp-row="' + i + '"]');
    if (!emp || !row) return;
    const cell = row.children[1]; // 部门列
    const sel = document.createElement("select");
    sel.className = "input";
    sel.style.cssText = "padding:4px 8px;font-size:12px;width:auto";
    sel.innerHTML = '<option value="">未分配</option>' +
      D.DEPARTMENTS.map((d) => '<option value="' + esc(d.name) + '"' + (emp.dept === d.name ? " selected" : "") + ">" + esc(d.name) + "</option>").join("");
    const ok = document.createElement("button");
    ok.className = "btn btn-primary";
    ok.style.cssText = "padding:4px 10px;font-size:12px";
    ok.textContent = "确认";
    const cancel = document.createElement("button");
    cancel.className = "btn btn-ghost";
    cancel.style.cssText = "padding:4px 10px;font-size:12px";
    cancel.textContent = "取消";
    const wrap = document.createElement("span");
    wrap.style.cssText = "display:inline-flex;gap:6px;align-items:center";
    wrap.append(sel, ok, cancel);
    cell.innerHTML = "";
    cell.appendChild(wrap);
    sel.focus();
    const done = () => {
      const v = sel.value;
      if (v !== emp.dept) {
        emp.dept = v;
        renderAdmin();
        toast("已把 " + emp.name + " 调整到 " + (v ? v + " 部门" : "未分配"));
      } else {
        renderAdmin();
      }
    };
    ok.addEventListener("click", done);
    cancel.addEventListener("click", () => renderAdmin());
    sel.addEventListener("change", () => ok.focus());
  }

  // 部门已用/成员数由 EMPLOYEES 实时汇总，部门改名/删改后自动联动
  function deptMemberCount(name) {
    return D.EMPLOYEES.filter((e) => e.dept === name).length;
  }

  function deptUsed(d) {
    return D.EMPLOYEES.filter((e) => e.dept === d.name).reduce((a, e) => a + e.used, 0);
  }

  function renderOrg() {
    const q = ($("#od-search").value || "").toLowerCase();
    const list = D.DEPARTMENTS.filter((d) => !q || d.name.toLowerCase().includes(q));

    const totalQuota = D.DEPARTMENTS.reduce((a, d) => a + d.quota, 0);
    const totalUsed = D.DEPARTMENTS.reduce((a, d) => a + deptUsed(d), 0);
    const unassigned = D.EMPLOYEES.filter((e) => !e.dept).length;
    $("#dept-stats").innerHTML = [
      stat("部门数 Departments", D.DEPARTMENTS.length + " 个", unassigned ? "未分配 " + unassigned + " 人" : "全部部门"),
      stat("月度总分配 Monthly quota", D.fmt(totalQuota) + " 点", "按月分配"),
      stat("已用 Used", D.fmt(totalUsed) + " 点", totalQuota ? Math.round((totalUsed / totalQuota) * 100) + "% 消耗率" : "—"),
      stat("剩余 Remain", D.fmt(totalQuota - totalUsed) + " 点", "按部门分配"),
    ].join("");

    $("#dept-body").innerHTML = list.length ? list.map((d, i) => {
      const used = deptUsed(d);
      const pct = d.quota > 0 ? used / d.quota : 0;
      const st = pct >= 1 ? '<span class="badge danger">已用尽</span>' : pct > 0.9 ? '<span class="badge warn">接近限额</span>' : '<span class="badge ok">正常</span>';
      return "<tr><td><strong>" + esc(d.name) + "</strong></td>" +
        "<td>" + deptMemberCount(d.name) + " 人</td>" +
        "<td>" + D.fmt(d.quota) + " 点</td>" +
        "<td>" + D.fmt(used) + " 点</td>" +
        "<td>" + D.fmt(d.quota - used) + " 点</td>" +
        "<td>" + st + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-dept-edit='" + i + "'>编辑</button> " +
        "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-dept-del='" + i + "'>删除</button></td></tr>";
    }).join("") : '<tr><td colspan="7" class="muted">没有匹配的部门</td></tr>';
  }

  // 月分配点数必须为正整数
  function promptPositiveInt(promptText, fallback) {
    const raw = window.prompt(promptText, fallback);
    if (raw == null) return null; // 取消
    const s = String(raw).trim();
    const n = Number(s);
    if (!s || !Number.isInteger(n) || n <= 0) { toast("请输入正整数点数（未生效）"); return null; }
    return n;
  }

  function addDept() {
    const name = window.prompt("新部门名称：", "");
    if (name == null) return;
    const n = String(name).trim();
    if (!n) { toast("请输入部门名称（未生效）"); return; }
    if (D.DEPARTMENTS.some((d) => d.name === n)) { toast("部门「" + n + "」已存在"); return; }
    const quota = promptPositiveInt("每月点数分配（正整数）：", "10000");
    if (quota == null) return;
    D.DEPARTMENTS.push({ id: Date.now(), name: n, quota });
    renderAdmin();
    toast("已添加部门「" + n + "」（月分配 " + D.fmt(quota) + " 点）");
  }

  function editDept(i) {
    const d = D.DEPARTMENTS[i];
    if (!d) return;
    const name = window.prompt("修改部门名称：", d.name);
    if (name == null) return;
    const n = String(name).trim();
    if (!n) { toast("部门名称不能为空（未生效）"); return; }
    if (n !== d.name && D.DEPARTMENTS.some((x) => x.name === n)) { toast("部门「" + n + "」已存在"); return; }
    const quota = promptPositiveInt("每月点数分配（正整数）：", String(d.quota));
    if (quota == null) return;
    const old = d.name;
    d.name = n;
    d.quota = quota;
    if (n !== old) D.EMPLOYEES.forEach((e) => { if (e.dept === old) e.dept = n; }); // 成员部门联动改名
    renderAdmin();
    toast("已更新部门「" + n + "」（月分配 " + D.fmt(quota) + " 点）");
  }

  function deleteDept(i) {
    const d = D.DEPARTMENTS[i];
    if (!d) return;
    const members = deptMemberCount(d.name);
    const msg = members > 0
      ? "该部门有 " + members + " 名成员，删除后这些成员将失去部门归属。确认删除部门「" + d.name + "」？"
      : "确认删除部门「" + d.name + "」？";
    if (!window.confirm(msg)) return;
    D.DEPARTMENTS.splice(i, 1);
    renderAdmin();
    toast("已删除部门「" + d.name + "」");
  }

  function barRow(name, pts, max, unit) {
    const pct = Math.round((pts / max) * 100);
    return '<div class="bar-row"><div class="bar-label"><span>' + esc(name) + '</span><span class="n">' + D.fmt(pts) + " " + unit + "</span></div>" +
      '<div class="bar"><i style="width:' + pct + '%"></i></div></div>';
  }

  /* ---------------- 事件 ---------------- */

  function bindEvents() {
    // 登录（单一入口，角色由账号决定）
    $("#login-form").addEventListener("submit", (e) => {
      e.preventDefault();
      $("#login-view").classList.add("hidden");
      $("#app").classList.remove("hidden");
      switchView("dashboard");
      toast("欢迎回来，阿零（演示账号）");
    });

    $("#logout-btn").addEventListener("click", () => {
      $("#app").classList.add("hidden");
      $("#login-view").classList.remove("hidden");
      toast("已退出（静态演示）");
    });

    // 市场筛选
    $("#mk-search").addEventListener("input", renderMarketplace);
    $("#mk-provider").addEventListener("change", renderMarketplace);
    $("#mk-sort").addEventListener("change", renderMarketplace);
    if (!$("#mk-provider").dataset.init) {
      $("#mk-provider").innerHTML = '<option value="">全部厂商</option>' + D.PROVIDERS.map((p) => '<option value="' + p + '">' + p + "</option>").join("");
      $("#mk-provider").dataset.init = "1";
    }

    // 共享上架表单（默认收起；点添加展开，提交成功或取消后收起）
    const shareFormCard = () => $("#share-form-card");
    const showShareForm = () => { shareFormCard().hidden = false; $("#sf-key").focus(); };
    const hideShareForm = () => { shareFormCard().hidden = true; };

    $("#share-add-btn").addEventListener("click", showShareForm);
    $("#sf-cancel").addEventListener("click", hideShareForm);

    // 共享上架表单（须填 API Key；单价由平台按模型定价自动计算）
    $("#share-form").addEventListener("submit", (e) => {
      e.preventDefault();
      const model = $("#sf-model").value;
      const quota = Number($("#sf-quota").value || 0);
      const key = $("#sf-key").value.trim();
      if (!key) { toast("请填写 API Key（上架 key 必须提供真实密钥）"); return; }
      if (!model || quota <= 0) { toast("请选择模型并填写有效额度"); return; }
      const price = autoPrice(model);
      D.SHARINGS.unshift({ id: Date.now(), model, quota, used: 0, price, earned: 0, status: "on", key });
      renderSharing();
      if (activeView === "dashboard") renderDashboard();
      toast("已上架 " + model + "（key 已加密托管，单价 " + D.fmt(price) + " 点/1M 自动）");
      e.target.reset();
      const p = $("#sf-provider"); p.value = ""; p.dispatchEvent(new Event("change"));
      $("#sf-quota").value = 5000;
      hideShareForm();
    });

    // 共享列表操作（事件委托：暂停/恢复/重新上架 + 删除）
    $("#share-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-share-toggle]");
      if (b) { toggleSharing(Number(b.dataset.shareToggle)); return; }
      const d = e.target.closest("[data-share-delete]");
      if (d) deleteSharing(Number(d.dataset.shareDelete));
    });

    // 钱包按钮（充值/提现 disabled + pointer-events:none，点击不生效；文案见 index.html wallet-note）
    // 钱包页提示 → 跳转交易记录（明细统一入口）
    $("#wallet-goto-tx").addEventListener("click", () => switchView("transactions"));

    // 交易 Tab
    $$("#tx-tabs .tab").forEach((b) => b.addEventListener("click", () => { txTab = b.dataset.txTab; txTable.page = 1; renderTransactions(); }));

    // API Key 生成（带名字；列表展示脱敏、复制给完整 id）
    $("#new-api-key-btn").addEventListener("click", () => {
      const input = window.prompt("新 key 名字（留空则默认「未命名」）：", "未命名");
      if (input == null) return;
      const name = String(input).trim() || "未命名";
      const id = "atk_live_" + Array.from({ length: 12 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
      D.API_KEYS.unshift({ id, name, created: "2026-08-14", last: "从未", status: "active" });
      renderSettings();
      toast("已生成新 API Key「" + name + "」（完整 key 已展示在列表，可复制）");
    });

    // API Key 搜索 + 行内操作（复制 / 改名 / 删除）
    $("#ak-search").addEventListener("input", renderSettings);

    $("#api-keys").addEventListener("click", (e) => {
      const cp = e.target.closest("[data-key-copy]");
      if (cp) { copyKey(Number(cp.dataset.keyCopy)); return; }
      const rn = e.target.closest("[data-key-rename]");
      if (rn) { renameKey(Number(rn.dataset.keyRename)); return; }
      const dl = e.target.closest("[data-key-del]");
      if (dl) deleteKey(Number(dl.dataset.keyDel));
    });

    // 管理台 Tabs
    $$("#admin-tabs .tab").forEach((b) => b.addEventListener("click", () => {
      $$("#admin-tabs .tab").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      renderAdmin();
    }));

    $("#add-key-btn").addEventListener("click", () => {
      const m = D.MODELS[Math.floor(Math.random() * D.MODELS.length)];
      D.KEYS.unshift({ id: Date.now(), provider: m.provider, model: m.model, key: "sk-****-new1", quota: 100000, used: 0, status: "ok" });
      renderAdmin();
      toast("已添加上游 Key（演示：" + m.model + "）");
    });

    $("#ak-search").addEventListener("input", renderAdmin);

    // 管理台事件委托（撤销 key / 员工充值）
    $("#keys-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-key-revoke]");
      if (!b) return;
      const k = D.KEYS.find((x) => x.id === Number(b.dataset.keyRevoke));
      if (!k) return;
      k.status = k.status === "revoked" ? "ok" : "revoked";
      renderAdmin();
      toast(k.status === "revoked" ? "已撤销 " + k.model + " 的 key" : "已恢复 " + k.model + " 的 key");
    });

    $("#emp-body").addEventListener("click", (e) => {
      const dd = e.target.closest("[data-emp-dept]");
      if (dd) { editEmpDept(Number(dd.dataset.empDept)); return; }
      const b = e.target.closest("[data-emp-topup]");
      if (!b) return;
      const emp = D.EMPLOYEES[Number(b.dataset.empTopup)];
      if (!emp) return;
      const input = prompt("为 " + emp.name + " 充值点数（正整数，任意金额）：", "5000");
      if (input == null) return; // 用户取消
      const raw = String(input).trim();
      const amt = Number(raw);
      if (!raw || !Number.isInteger(amt) || amt <= 0) {
        toast("请输入正整数点数金额（未生效）");
        return;
      }
      emp.quota += amt;
      renderAdmin();
      toast("已给 " + emp.name + " 充值 " + D.fmt(amt) + " 点");
    });

    // 组织管理：部门搜索 / 添加 / 编辑 / 删除（事件委托）
    $("#od-search").addEventListener("input", renderAdmin);

    $("#add-dept-btn").addEventListener("click", addDept);

    $("#dept-body").addEventListener("click", (e) => {
      const ed = e.target.closest("[data-dept-edit]");
      if (ed) { editDept(Number(ed.dataset.deptEdit)); return; }
      const dl = e.target.closest("[data-dept-del]");
      if (dl) deleteDept(Number(dl.dataset.deptDel));
    });
  }

  /* ---------------- 初始化 ---------------- */

  document.addEventListener("DOMContentLoaded", () => {
    renderNav();
    bindEvents();
    renderView("dashboard");
    $("#side-balance").textContent = D.fmt(D.USER.balance);
  });
})();
