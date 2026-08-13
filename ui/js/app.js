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
  let txTab = "all", txPage = 1, walletPage = 1;
  const TX_PAGE = 5;

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
      stat("点数余额 Points", D.fmt(D.USER.balance), "1 USD = 1000 点", "accent"),
      stat("本月用量 Usage", D.fmt(monthUse) + " 点", "共 " + txs.filter((t) => t.type === "consume").length + " 笔消费"),
      stat("共享收益 Earnings", "+" + D.fmt(monthEarn) + " 点", D.SHARINGS.filter((s) => s.status === "on").length + " 个 key 上架中"),
      stat("交易笔数 Trades", txs.length + " 笔", "含充值 / 提现 / 消费 / 收益"),
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
      };
      selP.addEventListener("change", fillModels);
      selP.dataset.init = "1";
      fillModels();
    }

    $("#share-body").innerHTML = D.SHARINGS.map((s, i) =>
      "<tr><td><strong>" + esc(s.model) + "</strong></td>" +
      "<td>" + D.fmt(s.used) + " / " + D.fmt(s.quota) + "</td>" +
      "<td>" + D.fmt(s.price) + " 点/1M</td>" +
      "<td>+" + D.fmt(s.earned) + " 点</td>" +
      "<td>" + badge(s.status, SHARE_STATUS) + "</td>" +
      "<td><button class='btn btn-ghost' data-share-toggle='" + i + "' style='padding:4px 10px;font-size:12px'>" +
      (s.status === "on" ? "暂停" : s.status === "paused" ? "恢复" : "重新上架") + "</button></td></tr>"
    ).join("");
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
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    $("#wallet-balance").textContent = D.fmt(D.USER.balance);

    const list = D.TRANSACTIONS;
    const pages = Math.max(1, Math.ceil(list.length / TX_PAGE));
    if (walletPage > pages) walletPage = pages;
    const rows = list.slice((walletPage - 1) * TX_PAGE, walletPage * TX_PAGE);

    $("#wallet-body").innerHTML = rows.map((t) =>
      "<tr><td>" + esc(t.time) + "</td><td>" + esc(t.typeName || t.type) + "</td>" +
      "<td>" + esc(t.partner) + "</td><td>" + esc(t.detail) + "</td>" +
      "<td style='color:" + (t.pts > 0 ? "var(--ok)" : "var(--text)") + ";font-weight:600'>" +
      (t.pts > 0 ? "+" : "") + D.fmt(t.pts) + "</td></tr>"
    ).join("");

    pager(pages, walletPage, (p) => { walletPage = p; renderWallet(); }, $("#wallet-pager"));
  }

  /* --- 交易记录 --- */

  const TX_TYPE = {
    consume: "消费", earn: "收益", topup: "充值", withdraw: "提现",
  };

  function renderTransactions() {
    $$("#tx-tabs .tab").forEach((b) => b.classList.toggle("active", b.dataset.txTab === txTab));
    let list = D.TRANSACTIONS;
    if (txTab === "consume") list = list.filter((t) => t.type === "consume");
    else if (txTab === "earn") list = list.filter((t) => t.type === "earn");

    const pages = Math.max(1, Math.ceil(list.length / TX_PAGE));
    if (txPage > pages) txPage = pages;
    const rows = list.slice((txPage - 1) * TX_PAGE, txPage * TX_PAGE);

    $("#tx-body").innerHTML = rows.map((t) =>
      "<tr><td>" + esc(t.time) + "</td>" +
      "<td>" + (t.type === "earn" ? '<span class="badge ok">收益</span>' : t.type === "consume" ? '<span class="badge accent">消费</span>' : '<span class="badge dim">' + esc(TX_TYPE[t.type] || t.type) + "</span>") + "</td>" +
      "<td>" + esc(t.partner) + "</td><td>" + esc(t.tokens) + "</td>" +
      "<td style='color:" + (t.pts > 0 ? "var(--ok)" : "var(--text)") + ";font-weight:600'>" + (t.pts > 0 ? "+" : "") + D.fmt(t.pts) + "</td>" +
      "<td>" + (t.status === "处理中" ? '<span class="badge warn">' + esc(t.status) + "</span>" : esc(t.status)) + "</td></tr>"
    ).join("");

    pager(pages, txPage, (p) => { txPage = p; renderTransactions(); }, $("#tx-pager"));
  }

  function pager(pages, cur, go, container) {
    if (pages <= 1) { container.innerHTML = ""; return; }
    let html = "";
    for (let i = 1; i <= pages; i++) {
      html += '<button class="' + (i === cur ? "active" : "") + '" data-p="' + i + '">' + i + "</button>";
    }
    html += "<span>" + cur + " / " + pages + "</span>";
    container.innerHTML = html;
    Array.from(container.querySelectorAll("button[data-p]")).forEach((b) =>
      b.addEventListener("click", () => go(Number(b.dataset.p))));
  }

  /* --- 设置 --- */

  function renderSettings() {
    $("#api-keys").innerHTML = D.API_KEYS.map((k) =>
      '<div class="mini-item"><div><div class="t">' + esc(k.name) + "</div>" +
      '<div class="d"><code>' + esc(k.id) + "</code> · 创建于 " + esc(k.created) + "</div></div>" +
      '<div class="r"><span class="d">最近使用 ' + esc(k.last) + "</span></div></div>"
    ).join("");
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
      $("#emp-stats").innerHTML = [
        stat("成员数 Members", D.EMPLOYEES.length + " 人", "2 个部门"),
        stat("总配额 Quota", D.fmt(total) + " 点", "月"),
        stat("已用 Usage", D.fmt(used) + " 点", Math.round((used / total) * 100) + "% 消耗率"),
        stat("剩余 Remain", D.fmt(total - used) + " 点", "按成员分配"),
      ].join("");
      $("#emp-body").innerHTML = D.EMPLOYEES.map((e, i) =>
        "<tr><td><strong>" + esc(e.name) + "</strong></td><td>" + esc(e.dept) + "</td>" +
        "<td>" + D.fmt(e.used) + " / " + D.fmt(e.quota) + "</td>" +
        "<td>" + D.fmt(e.quota - e.used) + "</td>" +
        "<td>" + (e.used / e.quota > 0.9 ? '<span class="badge warn">接近限额</span>' : '<span class="badge ok">正常</span>') + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-add='" + i + "'>+ 5000 点</button></td></tr>"
      ).join("");
    } else if (tab === "usage") {
      const maxM = Math.max(...D.USAGE_MODEL.map((u) => u.pts));
      const maxE = Math.max(...D.USAGE_EMP.map((u) => u.pts));
      $("#usage-model").innerHTML = D.USAGE_MODEL.map((u) => barRow(u.name, u.pts, maxM, "点")).join("");
      $("#usage-emp").innerHTML = D.USAGE_EMP.map((u) => barRow(u.name, u.pts, maxE, "点")).join("");
    }
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

    $("#sso-btn").addEventListener("click", () => toast("企业 SSO 为占位入口（静态原型）"));

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

    // 共享上架表单
    $("#share-form").addEventListener("submit", (e) => {
      e.preventDefault();
      const model = $("#sf-model").value;
      const quota = Number($("#sf-quota").value || 0);
      const price = Number($("#sf-price").value || 0);
      if (!model || quota <= 0) { toast("请选择模型并填写有效额度"); return; }
      D.SHARINGS.unshift({ id: Date.now(), model, quota, used: 0, price, earned: 0, status: "on" });
      renderSharing();
      if (activeView === "dashboard") renderDashboard();
      toast("已上架 " + model + "（额度 " + D.fmt(quota) + " 点）");
      e.target.reset();
      const p = $("#sf-provider"); p.value = ""; p.dispatchEvent(new Event("change"));
      $("#sf-quota").value = 5000; $("#sf-price").value = 280;
    });

    // 共享列表操作（事件委托）
    $("#share-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-share-toggle]");
      if (b) toggleSharing(Number(b.dataset.shareToggle));
    });

    // 钱包按钮
    $("#topup-btn").addEventListener("click", () => toast("充值入口为占位（静态原型）"));
    $("#withdraw-btn").addEventListener("click", () => toast("提现入口为占位（静态原型）"));

    // 交易 Tab
    $$("#tx-tabs .tab").forEach((b) => b.addEventListener("click", () => { txTab = b.dataset.txTab; txPage = 1; renderTransactions(); }));

    // API Key 生成
    $("#new-api-key-btn").addEventListener("click", () => {
      const id = "atk_live_" + Math.random().toString(16).slice(2, 10) + "…" + Math.random().toString(16).slice(2, 6);
      D.API_KEYS.unshift({ id, name: "新 Key（未命名）", created: "2026-08-13", last: "从未" });
      renderSettings();
      toast("已生成新 API Key（演示）");
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

    // 管理台事件委托（撤销 key / 员工加额）
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
      const b = e.target.closest("[data-emp-add]");
      if (!b) return;
      D.EMPLOYEES[Number(b.dataset.empAdd)].quota += 5000;
      renderAdmin();
      toast("已为成员 +5000 点配额");
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
