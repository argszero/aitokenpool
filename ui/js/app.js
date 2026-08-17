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
  let isGuest = false; // 游客模式（US-1：未登录可浏览市场）

  // MRT 风格表格状态（页面级变量：切换页面不丢失排序/筛选/分页）
  const txTable = { sort: [], filters: {}, page: 1, pageSize: 10 };

  /* ---------------- 工具 ---------------- */

  // toast 分级（rant 15:50:05 B.8：成功/错误/信息不同样式；默认 info）
  function toast(msg, type) {
    const el = $("#toast");
    el.textContent = msg;
    el.className = "toast" + (type ? " " + type : "");
    el.classList.remove("hidden");
    clearTimeout(toast._t);
    toast._t = setTimeout(() => el.classList.add("hidden"), 2600);
  }

  // 按钮 loading 态（rant 15:50:05 B.8：提交中转圈，模拟反馈后恢复）
  const SPINNER = '<span class="spin" aria-hidden="true"></span>';
  function withLoading(btn, fn, ms) {
    if (!btn || btn.dataset.loading) return;
    const orig = btn.innerHTML;
    btn.dataset.loading = "1";
    btn.disabled = true;
    btn.innerHTML = SPINNER + " 处理中…";
    setTimeout(() => {
      try { fn(); } finally {
        btn.dataset.loading = "";
        btn.disabled = false;
        btn.innerHTML = orig;
      }
    }, ms || 320);
  }

  /* --- 行内校验错误（rant 16:57:17 E：红边框 + 字段下方行内错误文案，修正后自动清除） --- */

  // 在输入框下方显示行内错误文案，并给输入框加红边框；输入修正时自动清除
  function setFieldError(input, msg) {
    if (!input) return;
    input.classList.add("input-error");
    let err = input.parentNode.querySelector(".field-error");
    if (!err) {
      err = document.createElement("span");
      err.className = "field-error";
      input.insertAdjacentElement("afterend", err);
    }
    err.textContent = msg;
    if (!input.dataset.errBound) {
      input.dataset.errBound = "1";
      input.addEventListener("input", () => clearFieldError(input), { once: false });
    }
  }

  function clearFieldError(input) {
    if (!input) return;
    input.classList.remove("input-error");
    const err = input.parentNode.querySelector(".field-error");
    if (err) err.textContent = "";
  }

  // 重新触发 tbody fade-in（rant 16:57:17 F：innerHTML 更新后重放动画，避免生硬闪烁）
  function pulseTbody(el) {
    if (!el) return;
    el.style.animation = "none";
    void el.offsetWidth; // 强制 reflow 以重启动画
    el.style.animation = "";
  }

  /* --- 行内二次确认 / 行内编辑（rant 16:57:17 A：清除原生确认/输入弹窗） --- */

  // 行内二次确认：首次点击按钮变「确认删除？」红色态，3 秒无操作或 Esc 还原，再次点击执行
  function confirmInline(btn, onConfirm, confirmText) {
    if (!btn) return;
    if (btn.dataset.confirm === "1") {
      clearTimeout(btn._confirmT);
      delete btn.dataset.confirm;
      btn.classList.remove("confirming");
      onConfirm();
      return;
    }
    btn.dataset.confirm = "1";
    const orig = btn.innerHTML;
    btn.innerHTML = confirmText || "确认删除？";
    btn.classList.add("confirming");
    btn._confirmT = setTimeout(() => revert(), 3000);
    const revert = () => {
      clearTimeout(btn._confirmT);
      if (btn.dataset.confirm === "1") delete btn.dataset.confirm;
      btn.classList.remove("confirming");
      btn.innerHTML = orig;
    };
    document.addEventListener("keydown", function esc(e) {
      if (e.key === "Escape") { revert(); document.removeEventListener("keydown", esc); }
    });
  }

  // 行内编辑表单：把容器替换为 input + 确认/取消，Enter 确认 / Esc 取消
  // opts: { value, placeholder, type, width, validate(val)->err|null, onSubmit(val), onCancel }
  function inlineForm(cell, opts) {
    const wrap = document.createElement("span");
    wrap.className = "inline-edit";
    wrap.style.cssText = "display:inline-flex;gap:6px;align-items:center";
    const input = document.createElement("input");
    input.type = opts.type || "text";
    input.className = "input";
    input.value = opts.value || "";
    input.placeholder = opts.placeholder || "";
    input.style.cssText = "padding:4px 8px;font-size:12px;width:" + (opts.width || "140px");
    const ok = document.createElement("button");
    ok.type = "button"; ok.className = "btn btn-primary"; ok.textContent = "确认";
    ok.style.cssText = "padding:4px 10px;font-size:12px";
    const cancel = document.createElement("button");
    cancel.type = "button"; cancel.className = "btn btn-ghost"; cancel.textContent = "取消";
    cancel.style.cssText = "padding:4px 10px;font-size:12px";
    wrap.append(input, ok, cancel);
    cell.innerHTML = "";
    cell.appendChild(wrap);
    input.focus();
    if (input.select) input.select();
    const finish = () => {
      const val = String(input.value).trim();
      const err = opts.validate ? opts.validate(val) : null;
      if (err) { setFieldError(input, err); return; }
      clearFieldError(input);
      opts.onSubmit(val);
    };
    ok.addEventListener("click", finish);
    cancel.addEventListener("click", opts.onCancel);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") finish();
      else if (e.key === "Escape") opts.onCancel();
    });
  }

  function badge(status, labels) {
    return '<span class="badge ' + (labels[status] ? labels[status].cls : "dim") + '">' +
      esc(labels[status] ? labels[status].text : status) + "</span>";
  }

  // 空状态组件（rant 15:50:05 A.4：列表/表格为空时给出图标 + 文案 + 可选行动按钮）
  const EMPTY_ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" class="es-ico"><path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.5 5h13l3.5 7v6a2 2 0 0 1-2 2h-16a2 2 0 0 1-2-2v-6l3.5-7z"/></svg>';

  function emptyState(text, sub, actionHtml) {
    return '<div class="empty-state">' + EMPTY_ICON + "<p>" + esc(text) + "</p>" +
      (sub ? '<p class="muted">' + esc(sub) + "</p>" : "") + (actionHtml || "") + "</div>";
  }

  function emptyRow(colspan, text, sub, actionHtml) {
    return '<tr><td colspan="' + colspan + '" class="empty-cell">' + emptyState(text, sub, actionHtml) + "</td></tr>";
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

  // Plan 提示：按量/订阅 + key 前缀 + 专属端点说明（来自 PLANS）
  function showPlanHint(planId) {
    const el = $("#sf-plan-hint");
    if (!el) return;
    const pl = D.PLANS.find((x) => x.id === planId);
    if (!pl) { el.textContent = ""; return; }
    el.textContent = (pl.type === "api" ? "按量计价的 key" : "订阅 Plan") +
      (pl.keyPrefix ? " · 建议 key 前缀 " + pl.keyPrefix : "") +
      (pl.note ? " · " + pl.note : "");
  }

  /* ---------------- 导航 ---------------- */

  // 统一内联 SVG 图标（线性风格、同尺寸、currentColor，替代 emoji；rant 15:50:05 A.2）
  const ICONS = {
    dashboard: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7.5" height="7.5" rx="1.5"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="1.5"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="1.5"/><rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1.5"/></svg>',
    marketplace: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="20" r="1.4"/><circle cx="17" cy="20" r="1.4"/><path d="M3 4h2l2.4 11.2a1.5 1.5 0 0 0 1.5 1.2h7.9a1.5 1.5 0 0 0 1.5-1.2L20 8H6"/></svg>',
    sharing: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10 14a5 5 0 0 0 7.1 0l3-3a5 5 0 0 0-7.1-7.1l-1.5 1.5"/><path d="M14 10a5 5 0 0 0-7.1 0l-3 3a5 5 0 0 0 7.1 7.1l1.5-1.5"/></svg>',
    wallet: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="6" width="18" height="14" rx="2"/><path d="M3 10h18"/><circle cx="16.5" cy="15" r="1.1" fill="currentColor" stroke="none"/></svg>',
    transactions: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3h12v18l-2-1.5L14 21l-2-1.5L10 21l-2-1.5L6 21V3z"/><path d="M9 8h6M9 12h6"/></svg>',
    admin: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6l7-3z"/><path d="M9.5 12l2 2 3.5-3.5"/></svg>',
    settings: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h13M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="19" cy="18" r="2"/></svg>',
  };

  const NAV = [
    { g: "主导航", items: [
      { id: "dashboard", icon: "dashboard", label: "仪表盘 Dashboard" },
      { id: "marketplace", icon: "marketplace", label: "模型市场 Marketplace" },
      { id: "sharing", icon: "sharing", label: "共享管理 Sharing" },
      { id: "wallet", icon: "wallet", label: "钱包 Wallet" },
      { id: "transactions", icon: "transactions", label: "交易记录 Transactions" },
    ]},
    { g: "角色视图", items: [
      { id: "admin", icon: "admin", label: "管理视图 Admin", role: "admin" },
      { id: "settings", icon: "settings", label: "设置 Settings" },
    ]},
  ];

  // 侧边栏视图顺序（rant 16:57:17 D：数字 1-7 切换对应视图，title 提示快捷键）
  const NAV_ORDER = NAV.flatMap((g) => g.items);

  const VIEW_TITLE = {
    dashboard: "仪表盘 Dashboard", marketplace: "模型市场 Marketplace", sharing: "共享管理 Sharing",
    wallet: "钱包 Wallet", transactions: "交易记录 Transactions", settings: "设置 Settings",
    admin: "管理视图 Admin",
  };

  // 游客可见的页面（US-1：仅市场；其余需登录）
  const GUEST_VIEWS = ["marketplace"];

  function renderNav() {
    const nav = $("#nav");
    nav.innerHTML = "";
    const groups = isGuest
      ? [{ g: "游客浏览", items: [
          { id: "marketplace", icon: "marketplace", label: "模型市场 Marketplace" },
        ]}]
      : NAV;
    groups.forEach((group) => {
      const g = document.createElement("div");
      g.className = "nav-group";
      g.textContent = group.g;
      nav.appendChild(g);
      group.items.forEach((item) => {
        const b = document.createElement("button");
        b.className = "nav-item" + (item.id === activeView ? " active" : "");
        b.dataset.view = item.id;
        const short = NAV_ORDER.indexOf(item) + 1; // 1-7
        b.title = "快捷键 " + short + " · " + item.label;
        b.innerHTML = '<span class="ico">' + (ICONS[item.icon] || "") + '</span><span class="label">' + esc(item.label) + "</span>" +
          (item.role ? "" : '<span class="nav-key">' + short + "</span>");
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
    $("#mode-label").textContent = isGuest ? "游客模式 · 仅浏览市场" : "共享市场 · 角色视图";
  }

  function switchView(id) {
    // 游客限制（US-1）：非市场页面 → 提示需登录
    if (isGuest && !GUEST_VIEWS.includes(id)) {
      toast("请先登录后再访问「" + (VIEW_TITLE[id] || id) + "」", "error");
      return;
    }
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

    $("#dash-stats").innerHTML = [
      stat("点数余额 Points", D.fmt(D.USER.balance), "", "accent"),
      stat("本月用量 Usage", D.fmt(monthUse) + " 点", "共 " + txs.filter((t) => t.type === "consume").length + " 笔消费"),
      stat("共享收益 Earnings", "+" + D.fmt(monthEarn) + " 点", D.SHARINGS.filter((s) => s.status === "on").length + " 个 key 上架中"),
      stat("交易笔数 Trades", txs.length + " 笔", "含充值 / 提现 / 消费 / 收益 / 赠送"),
    ].join("");

    const on = D.SHARINGS.filter((s) => s.status === "on");
    $("#dash-sharings").innerHTML = on.map((s) =>
      '<div class="mini-item"><div><div class="t">' + esc(s.model) + "</div>" +
      '<div class="d">' + esc(s.plan || "API") + " · 已用 " + D.fmt(s.used) + " / " + D.fmt(s.quota) + " 点 · 单价 " + D.fmt(s.price) + " 点/1M</div></div>" +
      '<div class="r"><span class="pts">+' + D.fmt(s.earned) + "</span><div class='d'>累计收益</div></div></div>"
    ).join("") + (on.length ? "" : '<div class="empty-state compact">' + EMPTY_ICON + '<p>还没有上架的 key</p><p class="muted">去「共享管理」把闲置 key 放进池子</p></div>');
    // 共享收益累计趋势 sparkline（rant 18:06:09 A；无上架 key 时保留空状态，不画图）
    if (on.length) {
      const days = lastDayLabels(7);
      const earn = dailySeries(days, (t) => t.type === "earn");
      let cum = 0;
      const cumSeries = earn.map((v) => { cum = Math.round((cum + v) * 100) / 100; return cum; });
      $("#dash-sharings").insertAdjacentHTML("afterbegin",
        sparkline(cumSeries, { labels: days, fmt: (v) => "+" + D.fmt(v), stroke: "var(--ok)" }));
    }
    renderMonthChanges();
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
      '<td class="num">' + D.fmt(m.in) + " 点</td>" + '<td class="num">' + D.fmt(m.out) + " 点</td>" +
      '<td class="num">' + D.ctxFmt(m.ctx) + "</td>" +
      "<td>" + (m.avail ? '<span class="badge ok">可用</span>' : '<span class="badge warn">繁忙</span>') +
      (m.multi ? ' <span class="badge ok" title="该模型配置多个上游 key，单个 key 不可用时自动故障转移（架构 v0.2 路由策略）">多 key · 自动故障转移</span>' : "") + "</td>" +
      "<td><button class='btn btn-primary' style='padding:4px 10px;font-size:12px' data-use-model='" + m.id + "'" + (m.avail ? "" : " disabled") + ">使用 / 消费</button>" +
      "<div class='muted' style='margin-top:4px;font-size:12px'>成功率 " + m.success + "%</div></td></tr>"
    ).join("") : emptyRow(7, "没有匹配的模型", "试试调整搜索或筛选条件",
      '<button type="button" class="btn btn-ghost" data-mk-clear-filters>清除筛选</button>');
    pulseTbody($("#mk-body"));
  }

  /* --- 可用时间段（rant 10:54:48：结构化字段，备注只作纯备注） --- */

  const DAY_LABELS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

  // 星期数字 → 展示文本：连续区间压缩为「周一~周五」，间断用 / 连接
  function fmtDays(nums) {
    const sorted = [...nums].sort((a, b) => a - b);
    const parts = [];
    let i = 0;
    while (i < sorted.length) {
      let j = i;
      while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++;
      parts.push(sorted[i] === sorted[j]
        ? DAY_LABELS[sorted[i] - 1]
        : DAY_LABELS[sorted[i] - 1] + "~" + DAY_LABELS[sorted[j] - 1]);
      i = j + 1;
    }
    return parts.join("/");
  }

  function fmtAvailable(s) {
    const a = s && s.available;
    if (!a || !a.days || !a.days.length) return "全天";
    const t = a.start && a.end ? " " + a.start + "-" + a.end : "";
    return fmtDays(a.days) + t;
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

    // 表单下拉（厂商 → Plan → 模型 三级联动；Plan 中「API」= 按量计价的 key）
    const selP = $("#sf-provider");
    if (!selP.dataset.init) {
      const planProviders = [...new Set(D.PLANS.map((pl) => pl.provider))];
      selP.innerHTML = '<option value="">选择厂商</option>' + planProviders
        .map((p) => '<option value="' + p + '">' + (D.PROVIDER_LABELS[p] || p) + "</option>").join("");
      const selPlan = $("#sf-plan");
      const selM = $("#sf-model");
      const fillModels = () => {
        const plan = D.PLANS.find((pl) => pl.id === selPlan.value);
        const p = plan ? plan.provider : selP.value;
        selM.innerHTML = '<option value="">选择模型</option>' + D.MODELS.filter((m) => !p || m.provider === p)
          .map((m) => '<option value="' + m.model + '">' + m.model + "</option>").join("");
        showPriceHint(selM.value);
      };
      const fillPlans = () => {
        const p = selP.value;
        selPlan.innerHTML = '<option value="">选择 Plan</option>' + D.PLANS.filter((pl) => pl.provider === p)
          .map((pl) => '<option value="' + pl.id + '">' + esc(pl.name) + "</option>").join("");
        showPlanHint("");
        fillModels();
      };
      selP.addEventListener("change", fillPlans);
      selPlan.addEventListener("change", () => { showPlanHint(selPlan.value); fillModels(); });
      selM.addEventListener("change", () => showPriceHint(selM.value));
      selP.dataset.init = "1";
      fillPlans();
    }

    $("#share-body").innerHTML = D.SHARINGS.length ? D.SHARINGS.map((s, i) =>
      "<tr><td><strong>" + esc(D.PROVIDER_LABELS[s.provider] || s.provider) + " · " + esc(s.plan || "API") +
      "</strong><div class='muted' style='font-size:12px'>" + esc(s.model) + " · " + esc(fmtAvailable(s)) + "</div></td>" +
      "<td class='mono'>" + esc(maskKey(s.key)) + "</td>" +
      "<td class='num'>" + D.fmt(s.used) + " / " + D.fmt(s.quota) + "</td>" +
      '<td class="num">' + D.fmt(s.price) + " 点/1M</td>" +
      '<td class="num">+' + D.fmt(s.earned) + " 点</td>" +
      "<td>" + timeCell(s.time) + "</td>" +
      "<td>" + badge(s.status, SHARE_STATUS) + "</td>" +
      "<td><button class='btn btn-ghost' data-share-toggle='" + i + "' style='padding:4px 10px;font-size:12px'>" +
      (s.status === "on" ? "暂停" : s.status === "paused" ? "恢复" : "重新上架") + "</button> " +
      "<button class='btn btn-danger' data-share-delete='" + i + "' style='padding:4px 10px;font-size:12px'>删除</button></td></tr>"
    ).join("") : emptyRow(8, "还没有上架的 key", "把闲置 key 放进池子，开始赚点数",
      '<button type="button" class="btn btn-primary" data-share-add>上架新 key</button>');
    pulseTbody($("#share-body"));
  }

  function deleteSharing(i) {
    const s = D.SHARINGS[i];
    if (!s) return;
    D.SHARINGS.splice(i, 1);
    renderSharing();
    if (activeView === "dashboard") renderDashboard();
    toast("已删除 " + s.model + " 的 key（彻底下架）", "success");
  }

  function toggleSharing(i) {
    const s = D.SHARINGS[i];
    if (s.status === "on") { s.status = "paused"; toast("已暂停 " + s.model + " 的共享", "success"); }
    else if (s.status === "paused") { s.status = "on"; toast("已恢复 " + s.model + " 的共享", "success"); }
    else { s.status = "on"; s.quota = 50000; s.used = 0; toast("已重新上架 " + s.model, "success"); }
    renderSharing();
    if (activeView === "dashboard") renderDashboard();
  }

  /* --- 本月点数变化（rant 10:45:27：近 1 月按类型汇总收支，取代静态"点数来源"分组） --- */

  const MONTH_TYPE_LABELS = [
    ["gift", "赠送"],
    ["expire", "过期"],
    ["earn", "收益"],
    ["consume", "消费"],
    ["topup", "充值"],
    ["withdraw", "提现"],
  ];

  function monthChangeItem(label, pts, isNet) {
    const sign = pts > 0 ? "+" : "";
    const neg = pts < 0 ? " neg" : "";
    return '<div class="mini-item' + (isNet ? " net" : "") + '"><div><div class="t">' + esc(label) + "</div></div>" +
      '<div class="r"><span class="pts' + neg + '">' + sign + D.fmt(pts) + "</span></div></div>";
  }

  /* --- 数据可视化（rant 18:06:09 A：纯 SVG 迷你折线图，零外部依赖） --- */

  let _sparkId = 0;

  // 生成 SVG sparkline：values 数值数组 → 折线 + 渐变填充，每点带 <title>（hover 显示当天数值）
  // opts: { labels: 与 values 等长的日期标签, fmt: 数值格式化, stroke, w, h }
  function sparkline(values, opts) {
    opts = opts || {};
    const w = opts.w || 120, h = opts.h || 34, pad = 2;
    const vals = values.length ? values : [0, 0];
    const max = Math.max.apply(null, vals.concat([0.0001]));
    const min = Math.min.apply(null, vals.concat([0]));
    const span = (max - min) || 1;
    const pts = vals.map((v, i) => {
      const x = vals.length <= 1 ? w / 2 : pad + (i * (w - pad * 2)) / (vals.length - 1);
      const y = h - pad - ((v - min) / span) * (h - pad * 2);
      return [x, y];
    });
    const line = pts.map((p, i) => (i ? "L" : "M") + p[0].toFixed(1) + " " + p[1].toFixed(1)).join(" ");
    const last = pts[pts.length - 1], first = pts[0];
    const area = line + " L" + last[0].toFixed(1) + " " + h + " L" + first[0].toFixed(1) + " " + h + " Z";
    const stroke = opts.stroke || "var(--accent)";
    const gid = "spark-grad-" + (++_sparkId);
    const fmt = opts.fmt || ((v) => v);
    const titles = pts.map((p, i) =>
      "<title>" + esc((opts.labels && opts.labels[i] ? opts.labels[i] + " " : "") + fmt(vals[i])) + "</title>").join("");
    return '<svg class="sparkline" viewBox="0 0 ' + w + " " + h + '" preserveAspectRatio="none" aria-hidden="true">' +
      "<defs><linearGradient id=\"" + gid + '" x1="0" y1="0" x2="0" y2="1">' +
      '<stop offset="0%" stop-color="' + stroke + '" stop-opacity="0.35"/>' +
      '<stop offset="100%" stop-color="' + stroke + '" stop-opacity="0"/>' +
      "</linearGradient></defs>" + titles +
      '<path d="' + area + '" fill="url(#' + gid + ')"/>' +
      '<path d="' + line + '" fill="none" stroke="' + stroke + '" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"/>' +
      "</svg>";
  }

  // 最近 n 天的日期标签（MM-DD），今天在前
  function lastDayLabels(n) {
    const p = (x) => String(x).padStart(2, "0");
    const out = [];
    for (let i = n - 1; i >= 0; i--) {
      const d = new Date();
      d.setDate(d.getDate() - i);
      out.push(p(d.getMonth() + 1) + "-" + p(d.getDate()));
    }
    return out;
  }

  // 按天聚合交易点数（filter 可选：只统计某类型），返回与 days 等长的序列
  function dailySeries(days, filter) {
    const map = {};
    D.TRANSACTIONS.forEach((t) => {
      if (filter && !filter(t)) return;
      const day = String(t.time || "").slice(0, 5);
      map[day] = (map[day] || 0) + t.pts;
    });
    return days.map((d) => Math.round((map[d] || 0) * 100) / 100);
  }

  function renderMonthChanges() {
    const sums = {};
    D.TRANSACTIONS.forEach((t) => { sums[t.type] = (sums[t.type] || 0) + t.pts; });
    const net = D.TRANSACTIONS.reduce((a, t) => a + t.pts, 0);
    const rows = MONTH_TYPE_LABELS
      .filter(([k]) => sums[k])
      .map(([k, label]) => monthChangeItem(label, sums[k], false));
    const html = monthChangeItem("本月净变化", net, true) +
      (rows.length ? rows.join("") : '<p class="muted">本月暂无变动</p>');
    const walletEl = $("#month-changes");
    if (walletEl) walletEl.innerHTML = html;
    const dashEl = $("#dash-month-changes");
    if (dashEl) {
      // 迷你折线图（rant 18:06:09 A：按天聚合净变化，hover 显示当天数值）
      const days = lastDayLabels(7);
      const net = dailySeries(days);
      dashEl.innerHTML = sparkline(net, { labels: days, fmt: (v) => (v > 0 ? "+" : "") + D.fmt(v) }) + html;
    }
  }

  /* --- 钱包 --- */

  function renderWallet() {
    // 钱包只做余额与资金操作；收支明细统一到【交易记录】（见 index.html wallet-hint）
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    $("#wallet-balance").textContent = D.fmt(D.USER.balance);
    // 本月点数变化（近 1 月按类型汇总收支，与仪表盘一致）
    renderMonthChanges();
  }

  /* --- 充值模拟（US-4：钱包页行内卡片 → 输入点数 → 余额增加 + topup 交易，永久有效点数） --- */

  function openTopup() {
    $("#topup-custom").value = "";
    $$("#topup-card .topup-presets .btn").forEach((b) => b.classList.remove("active"));
    $("#topup-card").hidden = false;
    $("#raise-card").hidden = true; // 互斥：开充值收起加额
    clearFieldError($("#topup-custom"));
    $("#topup-custom").focus();
  }

  function closeTopup() {
    $("#topup-card").hidden = true;
  }

  function confirmTopup() {
    const preset = document.querySelector("#topup-card .topup-presets .btn.active");
    const customRaw = $("#topup-custom").value;
    let amt;
    if (preset && !customRaw) amt = Number(preset.dataset.topupAmt);
    else {
      const raw = String(customRaw).trim();
      amt = Math.round(Number(raw) * 100) / 100;
      if (!raw || isNaN(amt) || amt <= 0) {
        setFieldError($("#topup-custom"), "请输入大于 0 的充值点数");
        return;
      }
    }
    clearFieldError($("#topup-custom"));
    D.USER.balance = Math.round((D.USER.balance + amt) * 100) / 100;
    D.TRANSACTIONS.unshift({
      id: Date.now(), time: nowTime(), type: "topup", partner: "—",
      detail: "充值 · 模拟支付（演示，真实支付后续接入）", tokens: "—", pts: +amt, status: "成功",
    });
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    renderWallet();
    closeTopup();
    toast("充值成功 +" + D.fmt(amt) + " 点（永久有效 · 演示）", "success");
  }

  /* --- 成员申请加额（US-20：余额低时申请更多点数 → 管理员审批开关联动） --- */

  function openRaise() {
    $("#raise-amount").value = "";
    $("#raise-reason").value = "";
    clearFieldError($("#raise-amount"));
    clearFieldError($("#raise-reason"));
    $("#raise-card").hidden = false;
    $("#topup-card").hidden = true; // 互斥：开加额收起充值
    $("#raise-amount").focus();
  }

  function closeRaise() {
    $("#raise-card").hidden = true;
  }

  function confirmRaise() {
    const rawAmt = String($("#raise-amount").value).trim();
    const reason = String($("#raise-reason").value).trim();
    const amt = Number(rawAmt);
    let firstErr = null;
    if (!rawAmt || !Number.isInteger(amt) || amt <= 0) { setFieldError($("#raise-amount"), "请输入正整数申请点数"); firstErr = firstErr || $("#raise-amount"); }
    else clearFieldError($("#raise-amount"));
    if (!reason) { setFieldError($("#raise-reason"), "请填写申请原因"); firstErr = firstErr || $("#raise-reason"); }
    else clearFieldError($("#raise-reason"));
    if (firstErr) { firstErr.focus(); return; }
    // 加额申请默认需管理员审批（原「需审批」开关随组织设置表单移除，见 rant 10:59:23）
    D.RAISE_REQUESTS.unshift({
      id: Date.now(), user: D.USER.name, email: D.USER.email, amount: amt, reason, status: "pending", time: nowTime(),
    });
    closeRaise();
    toast("已提交申请 +" + D.fmt(amt) + " 点，等待管理员审批", "success");
  }

  /* --- 管理员：加额申请审批（US-20） --- */

  const RAISE_STATUS = {
    pending: { text: "待审批", cls: "warn" },
    approved: { text: "已批准", cls: "ok" },
    rejected: { text: "已驳回", cls: "dim" },
  };

  function renderRaiseRequests() {
    const el = $("#raise-requests");
    if (!el) return;
    el.innerHTML = D.RAISE_REQUESTS.length ? '<div class="table-wrap compact"><table class="table"><thead><tr><th>成员</th><th class="num">申请点数</th><th>原因</th><th>状态</th><th></th></tr></thead><tbody>' +
      D.RAISE_REQUESTS.map((r, i) =>
        "<tr><td><strong>" + esc(r.user) + "</strong><div class='muted' style='font-size:12px'>" + esc(r.email) + "</div></td>" +
        "<td class='num'>+" + D.fmt(r.amount) + " 点</td>" +
        "<td>" + esc(r.reason) + "</td>" +
        "<td>" + badge(r.status, RAISE_STATUS) + "</td>" +
        "<td>" + (r.status === "pending"
          ? "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-raise-approve='" + i + "'>批准</button> " +
            "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-raise-reject='" + i + "'>驳回</button>"
          : '<span class="muted" style="font-size:12px">' + timeCell(r.time) + "</span>") + "</td></tr>"
      ).join("") + "</tbody></table></div>"
      : emptyState("暂无加额申请", "成员提交的加额申请会显示在这里，批准后自动加点数");
  }

  function approveRaise(i) {
    const r = D.RAISE_REQUESTS[i];
    if (!r || r.status !== "pending") return;
    r.status = "approved";
    // 批准 → 成员余额 + 申请点数（演示：若为当前登录用户则同步 D.USER.balance）
    if (r.email === D.USER.email) {
      D.USER.balance = Math.round((D.USER.balance + r.amount) * 100) / 100;
      D.TRANSACTIONS.unshift({
        id: Date.now(), time: nowTime(), type: "topup", partner: "管理员",
        detail: "加额 · 管理员批准申请（" + r.reason + "）", tokens: "—", pts: +r.amount, status: "成功",
      });
      $("#side-balance").textContent = D.fmt(D.USER.balance);
    }
    renderRaiseRequests();
    toast("已批准「" + r.user + "」申请 +" + D.fmt(r.amount) + " 点", "success");
  }

  function rejectRaise(i) {
    const r = D.RAISE_REQUESTS[i];
    if (!r || r.status !== "pending") return;
    r.status = "rejected";
    renderRaiseRequests();
    toast("已驳回「" + r.user + "」的加额申请", "success");
  }

  /* --- 交易记录 --- */

  const TX_TYPE = {
    consume: "消费", earn: "收益", topup: "充值", withdraw: "提现", gift: "赠送",
  };

  const TX_COLUMNS = [
    { key: "time", title: "时间", sort: "string", filter: "text", render: (t) => timeCell(t.time) },
    { key: "type", title: "类型", sort: "string", filter: "select", options: ["消费", "收益", "充值", "提现", "赠送"], filterVal: (t) => TX_TYPE[t.type] || t.type,
      render: (t) => t.type === "earn" ? '<span class="badge ok">收益</span>' : t.type === "consume" ? '<span class="badge accent">消费</span>' : t.type === "gift" ? '<span class="badge ok">赠送</span>' : '<span class="badge dim">' + esc(TX_TYPE[t.type] || t.type) + "</span>" },
    { key: "partner", title: "模型 / Key", sort: "string", filter: "text" },
    { key: "tokens", title: "Token 用量", sort: "string", filter: "text", align: "num" },
    { key: "pts", title: "点数", sort: "number", filter: "number-range", align: "num",
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
      html += '<th' + (col.align === "num" ? ' class="num"' : "") + '><button type="button" class="th-sort" data-sort-key="' + esc(col.key) + '" title="点击排序 · Shift+点击叠加多列">' +
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
    if (!pageRows.length) html += '<tr><td colspan="' + columns.length + '" class="empty-cell">' + emptyState("没有匹配的记录", "试试调整筛选条件") + "</td></tr>";
    pageRows.forEach((row) => {
      html += "<tr>";
      columns.forEach((col) => {
        html += "<td" + (col.align === "num" ? ' class="num"' : "") + ">" + (col.render ? col.render(row) : esc(row[col.key] == null ? "" : row[col.key])) + "</td>";
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
      "<td>" + timeCell(k.last) + "</td>" +
      "<td>" + (k.status === "active" ? '<span class="badge ok">启用</span>' : '<span class="badge dim">' + esc(k.status || "—") + "</span>") + "</td>" +
      "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-copy='" + i + "'>复制</button> " +
      "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-rename='" + i + "'>改名</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-key-del='" + i + "'>删除</button></td></tr>"
    ).join("") : emptyRow(6, "还没有 API Key", "生成一个 key，用于本地工具 / 代码接入平台",
      '<button type="button" class="btn btn-ghost" data-new-key>生成新 Key</button>');
    pulseTbody($("#api-keys"));
  }

  // 一键复制完整 key；file:// 下 clipboard API 受限 → 降级：临时 textarea 选中 + execCommand("copy")，仍失败则提示 Ctrl+C
  // 复制反馈（rant 15:50:05 B.10：复制后按钮短暂变「已复制」态）
  function copyKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    const btn = document.querySelector('[data-key-copy="' + i + '"]');
    const flash = (ok) => {
      if (!btn) return;
      const orig = btn.innerHTML;
      btn.disabled = true;
      btn.innerHTML = ok ? "已复制 ✓" : "请 Ctrl+C";
      setTimeout(() => { btn.disabled = false; btn.innerHTML = orig; }, 1200);
    };
    const okToast = () => { toast("已复制「" + k.name + "」完整 key 到剪贴板", "success"); flash(true); };
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
      else { toast("已选中完整 key，请按 Ctrl+C / Cmd+C 复制"); flash(false); }
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(k.id).then(okToast).catch(fallback);
    } else {
      fallback();
    }
  }

  // 生成新 API Key（行内编辑，替代原生输入弹窗，Enter 确认 / Esc 取消）
  function openNewKeyInline() {
    const wrap = $("#ak-new-inline");
    wrap.hidden = false;
    $("#ak-new-name").value = "";
    $("#ak-new-name").focus();
  }

  function closeNewKeyInline() {
    $("#ak-new-inline").hidden = true;
  }

  function commitNewKey() {
    const raw = String($("#ak-new-name").value).trim();
    const name = raw || "未命名";
    const id = "atk_live_" + Array.from({ length: 12 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
    D.API_KEYS.unshift({ id, name, created: "2026-08-14", last: "从未", status: "active" });
    renderSettings();
    closeNewKeyInline();
    toast("已生成新 API Key「" + name + "」（完整 key 已展示在列表，可复制）", "success");
  }

  // API Key 改名：行内编辑（替代原生输入弹窗）
  function renameKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    const row = document.querySelector('#api-keys tr:nth-child(' + (i + 1) + ')');
    const cell = row ? row.children[0] : null;
    if (!cell) return;
    inlineForm(cell, {
      value: k.name,
      placeholder: "key 名字",
      width: "160px",
      validate: (v) => v ? null : "名字不能为空",
      onSubmit: (name) => { k.name = name; renderSettings(); toast("已改名为「" + name + "」", "success"); },
      onCancel: () => renderSettings(),
    });
  }

  function deleteKey(i) {
    const k = D.API_KEYS[i];
    if (!k) return;
    D.API_KEYS.splice(i, 1);
    renderSettings();
    toast("已删除 key「" + k.name + "」", "success");
  }

  /* --- 管理员角色视图 --- */

  function renderAdmin() {
    const tab = $("#admin-tabs .tab.active").dataset.adminTab;
    $$(".admin-pane").forEach((p) => p.classList.toggle("hidden", p.dataset.adminPane !== tab));

    if (tab === "employees") {
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
        '<td class="num">' + D.fmt(e.used) + " / " + D.fmt(e.quota) + "</td>" +
        '<td class="num">' + D.fmt(e.quota - e.used) + "</td>" +
        "<td>" + (e.used / e.quota > 0.9 ? '<span class="badge warn">接近限额</span>' : '<span class="badge ok">正常</span>') + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-dept='" + i + "'>改部门</button> " +
        "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-topup='" + i + "'>充值</button></td></tr>"
      ).join("");
      pulseTbody($("#emp-body"));
      renderRaiseRequests();
    } else if (tab === "usage") {
      const maxM = Math.max(...D.USAGE_MODEL.map((u) => u.pts));
      const maxE = Math.max(...D.USAGE_EMP.map((u) => u.pts));
      $("#usage-model").innerHTML = D.USAGE_MODEL.map((u) => barRow(u.name, u.pts, maxM, "点")).join("");
      $("#usage-emp").innerHTML = D.USAGE_EMP.map((u) => barRow(u.name, u.pts, maxE, "点")).join("");
    } else if (tab === "org") {
      renderOrg();
    } else if (tab === "ops") {
      renderOperator();
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
        toast("已把 " + emp.name + " 调整到 " + (v ? v + " 部门" : "未分配"), "success");
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
        '<td class="num">' + deptMemberCount(d.name) + " 人</td>" +
        '<td class="num">' + D.fmt(d.quota) + " 点</td>" +
        '<td class="num">' + D.fmt(used) + " 点</td>" +
        '<td class="num">' + D.fmt(d.quota - used) + " 点</td>" +
        "<td>" + st + "</td>" +
        "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-dept-edit='" + i + "'>编辑</button> " +
        "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-dept-del='" + i + "'>删除</button></td></tr>";
    }).join("") : emptyRow(7, "没有匹配的部门", "试试调整搜索关键词",
      '<button type="button" class="btn btn-ghost" data-dept-clear-search>清除搜索</button>');
    pulseTbody($("#dept-body"));
  }

  /* --- 部门添加/编辑：行内展开表单（UI 原则：少用弹窗，优先行内交互；继承 rant 10:59:47 的可靠响应） --- */

  let deptEditIndex = null; // null = 添加，数字 = 编辑的部门索引

  function openDeptForm(i) {
    deptEditIndex = (i == null ? null : i);
    const d = (i == null ? null : D.DEPARTMENTS[i]);
    $("#dept-form-title").innerHTML = d
      ? "编辑部门 <span class='en'>Edit department</span>"
      : "添加部门 <span class='en'>Add department</span>";
    $("#dept-form-name").value = d ? d.name : "";
    $("#dept-form-quota").value = d ? String(d.quota) : "";
    clearFieldError($("#dept-form-name"));
    clearFieldError($("#dept-form-quota"));
    $("#dept-form-card").hidden = false;
    $("#dept-form-name").focus();
  }

  function confirmDept() {
    const name = String($("#dept-form-name").value).trim();
    const rawQ = String($("#dept-form-quota").value).trim();
    const quota = Number(rawQ);
    let firstErr = null;
    if (!name) { setFieldError($("#dept-form-name"), "请输入部门名称"); firstErr = firstErr || $("#dept-form-name"); }
    else clearFieldError($("#dept-form-name"));
    if (!rawQ || !Number.isInteger(quota) || quota <= 0) { setFieldError($("#dept-form-quota"), "请输入正整数月分配点数"); firstErr = firstErr || $("#dept-form-quota"); }
    else clearFieldError($("#dept-form-quota"));
    if (firstErr) { firstErr.focus(); return; }
    if (deptEditIndex == null) {
      if (D.DEPARTMENTS.some((d) => d.name === name)) { setFieldError($("#dept-form-name"), "部门「" + name + "」已存在"); $("#dept-form-name").focus(); return; }
      D.DEPARTMENTS.push({ id: Date.now(), name, quota });
      toast("已添加部门「" + name + "」（月分配 " + D.fmt(quota) + " 点）", "success");
    } else {
      const d = D.DEPARTMENTS[deptEditIndex];
      if (!d) return;
      if (name !== d.name && D.DEPARTMENTS.some((x) => x.name === name)) { setFieldError($("#dept-form-name"), "部门「" + name + "」已存在"); $("#dept-form-name").focus(); return; }
      const old = d.name;
      d.name = name;
      d.quota = quota;
      if (name !== old) D.EMPLOYEES.forEach((e) => { if (e.dept === old) e.dept = name; }); // 成员部门联动改名
      toast("已更新部门「" + name + "」（月分配 " + D.fmt(quota) + " 点）", "success");
    }
    renderAdmin();
    $("#dept-form-card").hidden = true;
  }

  function deleteDept(i) {
    const d = D.DEPARTMENTS[i];
    if (!d) return;
    D.DEPARTMENTS.splice(i, 1);
    renderAdmin();
    toast("已删除部门「" + d.name + "」", "success");
  }

  function barRow(name, pts, max, unit) {
    const pct = Math.round((pts / max) * 100);
    return '<div class="bar-row"><div class="bar-label"><span>' + esc(name) + '</span><span class="n">' + D.fmt(pts) + " " + unit + "</span></div>" +
      '<div class="bar"><i style="width:' + pct + '%"></i></div></div>';
  }

  /* --- 平台运营者视图（US-运营1 / US-运营2：运营者 = 宿主本人，职责仅两项） --- */

  function renderOperator() {
    const q = ($("#ops-search").value || "").toLowerCase();
    const list = D.OPERATOR_USERS.filter((u) => !q || u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q));
    const txs = D.TRANSACTIONS;
    const flowIn = txs.filter((t) => t.pts > 0).reduce((a, t) => a + t.pts, 0);
    const flowOut = txs.filter((t) => t.pts < 0).reduce((a, t) => a + Math.abs(t.pts), 0);
    const onKeys = D.SHARINGS.filter((s) => s.status === "on").length;

    $("#ops-stats").innerHTML = [
      stat("运行状态 Status", '<span class="badge ok">在线</span>', "平台服务正常"),
      stat("用户数 Users", D.OPERATOR_USERS.length + " 人", "注册用户"),
      stat("共享 key 数 Keys", onKeys + " 个", D.SHARINGS.length + " 个历史上架"),
      stat("交易量 Trades", txs.length + " 笔", "累计全部类型"),
      stat("点数流入 In", "+" + D.fmt(flowIn) + " 点", "收益 / 充值 / 赠送"),
      stat("点数流出 Out", "-" + D.fmt(flowOut) + " 点", "消费 / 提现"),
    ].join("");

    $("#ops-body").innerHTML = list.length ? list.map((u) =>
      "<tr><td><strong>" + esc(u.name) + "</strong></td>" +
      "<td>" + esc(u.email) + "</td>" +
      '<td class="num">' + D.fmt(u.balance) + " 点</td>" +
      "<td><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-ops-topup='" + u.id + "'>充值点数</button></td></tr>"
    ).join("") : emptyRow(4, "没有匹配的用户", "试试其他用户名 / 邮箱");
    pulseTbody($("#ops-body"));
  }

  // 运营者给用户充值：行内编辑（替代原生输入弹窗，Enter 确认 / Esc 取消）
  function inlineOpsTopup(u, btn) {
    const row = btn.closest("tr");
    if (!row) return;
    const cell = row.children[3];
    inlineForm(cell, {
      value: "100",
      placeholder: "点数（可为小数）",
      type: "number",
      width: "120px",
      validate: (raw) => {
        const amt = Math.round(Number(raw) * 100) / 100;
        return (!raw || isNaN(amt) || amt <= 0) ? "请输入大于 0 的点数金额" : null;
      },
      onSubmit: (raw) => {
        const amt = Math.round(Number(raw) * 100) / 100;
        u.balance = Math.round((u.balance + amt) * 100) / 100;
        if (u.email === D.USER.email) D.USER.balance = u.balance;
        D.TRANSACTIONS.unshift({
          id: Date.now(), time: nowTime(), type: "topup", partner: "运营者",
          detail: "充值 · 运营者发放（永久有效）", tokens: "—", pts: amt, status: "成功",
        });
        renderAdmin();
        $("#side-balance").textContent = D.fmt(D.USER.balance);
        toast("已给「" + u.name + "」充值 " + D.fmt(amt) + " 点（永久有效）", "success");
      },
      onCancel: () => renderAdmin(),
    });
  }

  /* --- 消费模拟（US-6：市场页「使用 / 消费」→ 聊天 Mock，按模型参考价扣小数点数） --- */

  let chatModel = null;

  function nowTime() {
    const n = new Date();
    const p = (x) => String(x).padStart(2, "0");
    return p(n.getMonth() + 1) + "-" + p(n.getDate()) + " " + p(n.getHours()) + ":" + p(n.getMinutes());
  }

  // 相对时间（rant 16:57:17 B）：刚刚 / N 分钟前 / N 小时前 / 昨天 / MM-DD
  // 支持 "MM-DD HH:mm"（默认今年）与 "YYYY-MM-DD[ HH:mm]" 两种格式；非标准格式原样返回
  function timeAgo(s) {
    if (!s) return "";
    const p2 = (x) => String(x).padStart(2, "0");
    const full = String(s);
    const m = full.match(/^(\d{2})-(\d{2})\s+(\d{2}):(\d{2})$/) || full.match(/^(\d{4})-(\d{2})-(\d{2})(?:\s+(\d{2}):(\d{2}))?$/);
    if (!m) return full;
    const isFull = m[1].length === 4; // "YYYY-MM-DD" vs "MM-DD HH:mm"（分组语义不同）
    const MM = isFull ? +m[2] : +m[1];
    const DD = isFull ? +m[3] : +m[2];
    const HH = isFull ? +(m[4] || 0) : +m[3];
    const mm = isFull ? +(m[5] || 0) : +m[4];
    const now = new Date();
    const y = isFull ? +m[1] : now.getFullYear();
    const d = new Date(y, MM - 1, DD, HH, mm);
    if (isNaN(d.getTime())) return full;
    const min = Math.floor((now - d) / 60000);
    if (min < 1) return "刚刚";
    if (min < 60) return min + " 分钟前";
    if (min < 60 * 24) return Math.floor(min / 60) + " 小时前";
    const dayDiff = Math.floor(
      (new Date(now.getFullYear(), now.getMonth(), now.getDate()) - new Date(y, MM - 1, DD)) / 86400000);
    if (dayDiff === 1) return "昨天";
    return p2(MM) + "-" + p2(DD); // MM-DD
  }

  // 时间单元格：相对时间展示 + title 悬停显示完整绝对时间
  function timeCell(s) {
    if (!s) return "";
    return '<span class="timeago" title="' + esc(String(s)) + '">' + esc(timeAgo(s)) + "</span>";
  }

  function openChat(id) {
    const m = D.MARKET.find((x) => x.id === id);
    if (!m) return;
    if (!m.avail) { toast("该模型当前繁忙，无法使用", "error"); return; }
    chatModel = m;
    $("#chat-title").textContent = "使用 " + m.model;
    $("#chat-meta").textContent = "参考价：输入 " + D.fmt(m.in) + " 点 / 输出 " + D.fmt(m.out) + " 点（每 1M tokens）· 余额 " + D.fmt(D.USER.balance) + " 点" +
      (m.multi ? " · 多 key 自动故障转移" : "");
    $("#chat-log").innerHTML = '<p class="muted chat-tip">输入内容并发送，模拟一次模型调用（按参考价扣除小数点数，保留 2 位）</p>';
    $("#chat-input").value = "";
    $("#chat-modal").classList.remove("hidden");
    $("#chat-input").focus();
  }

  function closeChat() {
    $("#chat-modal").classList.add("hidden");
    chatModel = null;
  }

  function sendChat() {
    const m = chatModel;
    if (!m) return;
    const text = $("#chat-input").value.trim();
    if (!text) { toast("请输入消息内容", "error"); return; }
    // 模拟一次调用：0.19M tokens，按输出参考价计费（v1.6：消费点数可为小数，保留 2 位）
    const tokens = 0.19;
    const cost = Math.round(tokens * m.out * 100) / 100;
    if (D.USER.balance < cost) {
      toast("点数余额不足：本次约需 " + D.fmt(cost) + " 点（当前 " + D.fmt(D.USER.balance) + " 点）", "error");
      return;
    }
    D.USER.balance = Math.round((D.USER.balance - cost) * 100) / 100;
    D.TRANSACTIONS.unshift({
      id: Date.now(), time: nowTime(), type: "consume", partner: m.model,
      detail: "消费 · 聊天模拟（shared key）", tokens: "0.19M", pts: -cost, status: "成功",
    });
    const log = $("#chat-log");
    if (log.querySelector(".chat-tip")) log.innerHTML = "";
    log.innerHTML +=
      '<div class="chat-msg user"><div class="bubble">' + esc(text) + "</div></div>" +
      '<div class="chat-msg bot"><div class="bubble">（模拟回复）已收到你的请求。本次调用消耗约 0.19M tokens。</div></div>';
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    $("#chat-meta").textContent = "已扣 " + D.fmt(cost) + " 点（模拟 0.19M tokens）· 余额 " + D.fmt(D.USER.balance) + " 点";
    $("#chat-input").value = "";
    toast("已扣 " + D.fmt(cost) + " 点（模拟 0.19M tokens）", "success");
  }

  /* ---------------- 游客模式（US-1：未登录浏览市场） ---------------- */

  function enterGuest() {
    isGuest = true;
    activeView = "marketplace";
    $("#login-view").classList.add("hidden");
    $("#app").classList.remove("hidden");
    document.querySelector(".user-chip").classList.add("hidden");
    renderNav();
    switchView("marketplace");
    toast("游客模式：可浏览模型市场，使用 / 消费需登录", "info");
  }

  function exitGuest() {
    isGuest = false;
    $("#app").classList.add("hidden");
    document.querySelector(".user-chip").classList.remove("hidden");
    $("#login-view").classList.remove("hidden");
  }

  /* ---------------- 事件 ---------------- */

  function bindEvents() {
    // 游客浏览（US-1：登录页入口 → 免登录进入市场）
    $("#guest-browse-btn").addEventListener("click", enterGuest);

    // 登录（单一入口，角色由账号决定）
    $("#login-form").addEventListener("submit", (e) => {
      e.preventDefault();
      isGuest = false;
      document.querySelector(".user-chip").classList.remove("hidden");
      $("#login-view").classList.add("hidden");
      $("#app").classList.remove("hidden");
      switchView("dashboard");
      toast("欢迎回来，阿零（演示账号）", "info");
    });

    $("#logout-btn").addEventListener("click", () => {
      exitGuest();
      toast("已退出（静态演示）", "info");
    });

    // 市场筛选
    $("#mk-search").addEventListener("input", renderMarketplace);
    $("#mk-provider").addEventListener("change", renderMarketplace);
    $("#mk-sort").addEventListener("change", renderMarketplace);
    if (!$("#mk-provider").dataset.init) {
      $("#mk-provider").innerHTML = '<option value="">全部厂商</option>' + D.PROVIDERS.map((p) => '<option value="' + p + '">' + p + "</option>").join("");
      $("#mk-provider").dataset.init = "1";
    }

    // 市场页：使用 / 消费（G4：聊天 Mock 扣小数点数并产生 consume 交易；游客需先登录 US-1）
    $("#mk-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-use-model]");
      if (b) {
        if (isGuest) { toast("请先登录后再使用 / 消费模型", "error"); return; }
        openChat(Number(b.dataset.useModel));
        return;
      }
      // 空状态：清除筛选
      if (e.target.closest("[data-mk-clear-filters]")) {
        $("#mk-search").value = "";
        $("#mk-provider").value = "";
        $("#mk-sort").value = "default";
        renderMarketplace();
      }
    });
    $("#chat-send").addEventListener("click", sendChat);
    $("#chat-close").addEventListener("click", closeChat);
    $("#chat-input").addEventListener("keydown", (e) => { if (e.key === "Enter") sendChat(); });
    $("#chat-modal").addEventListener("click", (e) => { if (e.target === $("#chat-modal")) closeChat(); });

    // 平台运营者（G1 / US-运营2）：搜索定位用户 + 充值（行内编辑，永久有效点数，产生交易记录）
    $("#ops-search").addEventListener("input", renderAdmin);
    $("#ops-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-ops-topup]");
      if (!b) return;
      const u = D.OPERATOR_USERS.find((x) => x.id === Number(b.dataset.opsTopup));
      if (!u) return;
      inlineOpsTopup(u, b);
    });

    // 共享上架表单（默认收起；点添加展开，提交成功或取消后收起）
    const shareFormCard = () => $("#share-form-card");
    const showShareForm = () => {
      clearFieldError($("#sf-key"));
      clearFieldError($("#sf-quota"));
      shareFormCard().hidden = false;
      $("#sf-key").focus();
    };
    const hideShareForm = () => { shareFormCard().hidden = true; };

    $("#share-add-btn").addEventListener("click", showShareForm);
    $("#sf-cancel").addEventListener("click", hideShareForm);

    // 「每天」快捷选项：勾选 = 全选周一~周日（并禁用单日），取消 = 全清
    const allCb = document.querySelector("#sf-days-all input");
    if (allCb) allCb.addEventListener("change", () => {
      $$("#sf-days .chip input").forEach((cb) => {
        if (cb !== allCb) { cb.checked = allCb.checked; cb.disabled = allCb.checked; }
      });
    });

    // 共享上架表单（选 厂商 → Plan → 模型；单价由平台按模型定价自动计算）
    $("#share-form").addEventListener("submit", (e) => {
      e.preventDefault();
      const submitBtn = e.target.querySelector('button[type="submit"]');
      const done = () => {
        const model = $("#sf-model").value;
        const planId = $("#sf-plan").value;
        const plan = D.PLANS.find((pl) => pl.id === planId);
        const quota = Number($("#sf-quota").value || 0);
        const key = $("#sf-key").value.trim();
        const note = $("#sf-note").value.trim();
        let firstErr = null;
        if (!key) { setFieldError($("#sf-key"), "请填写 API Key（上架 key 必须提供真实密钥）"); firstErr = firstErr || $("#sf-key"); }
        else clearFieldError($("#sf-key"));
        if (!plan || !model || quota <= 0) {
          setFieldError($("#sf-quota"), "请选择厂商 / Plan / 模型并填写有效额度");
          firstErr = firstErr || $("#sf-quota");
        } else clearFieldError($("#sf-quota"));
        if (firstErr) { firstErr.focus(); return; }
        // 可用时间段：星期多选 + 起止时间；不选任何星期 = null（全天不限）
        const days = $$("#sf-days .chip input:not(#sf-days-all input)")
          .filter((cb) => cb.checked).map((cb) => Number(cb.value)).sort((a, b) => a - b);
        const start = $("#sf-start").value;
        const end = $("#sf-end").value;
        const available = days.length ? { days, start: start || "", end: end || "" } : null;
        const price = autoPrice(model);
        D.SHARINGS.unshift({ id: Date.now(), provider: plan.provider, plan: plan.name, model, quota, used: 0, price, earned: 0, status: "on", key, note, available });
        renderSharing();
        if (activeView === "dashboard") renderDashboard();
        const label = (D.PROVIDER_LABELS[plan.provider] || plan.provider) + " · " + plan.name;
        toast("已上架「" + label + " · " + model + "」（key 已加密托管，单价 " + D.fmt(price) + " 点/1M（自动））", "success");
        e.target.reset();
        const p = $("#sf-provider"); p.value = ""; p.dispatchEvent(new Event("change"));
        $("#sf-quota").value = 5000;
        hideShareForm();
      };
      withLoading(submitBtn, done);
    });

    // 共享列表操作（事件委托：暂停/恢复/重新上架 + 删除[行内二次确认] + 空状态上架）
    $("#share-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-share-toggle]");
      if (b) { toggleSharing(Number(b.dataset.shareToggle)); return; }
      const d = e.target.closest("[data-share-delete]");
      if (d) { confirmInline(d, () => deleteSharing(Number(d.dataset.shareDelete)), "确认彻底下架？"); return; }
      if (e.target.closest("[data-share-add]")) showShareForm();
    });

    // 钱包按钮（充值：US-4 行内卡片；申请加额：US-20 行内卡片；提现仍 disabled）
    $("#topup-btn").addEventListener("click", openTopup);
    $("#raise-btn").addEventListener("click", openRaise);
    $("#topup-confirm").addEventListener("click", (e) => withLoading(e.currentTarget, confirmTopup));
    $("#raise-confirm").addEventListener("click", (e) => withLoading(e.currentTarget, confirmRaise));
    $("#topup-cancel").addEventListener("click", closeTopup);
    $("#raise-cancel").addEventListener("click", closeRaise);
    // 键盘可达（rant 15:50:05 B.9）：Enter 提交、Esc 关闭行内编辑
    $("#topup-custom").addEventListener("keydown", (e) => { if (e.key === "Enter") $("#topup-confirm").click(); });
    $("#raise-amount").addEventListener("keydown", (e) => { if (e.key === "Enter") $("#raise-confirm").click(); });
    $("#raise-reason").addEventListener("keydown", (e) => { if (e.key === "Enter") $("#raise-confirm").click(); });
    ["topup-card", "raise-card"].forEach((id) => {
      document.getElementById(id).addEventListener("keydown", (e) => {
        if (e.key === "Escape") { document.getElementById(id).hidden = true; }
      });
    });
    $$("#topup-card .topup-presets .btn").forEach((b) =>
      b.addEventListener("click", () => {
        $$("#topup-card .topup-presets .btn").forEach((x) => x.classList.remove("active"));
        b.classList.add("active");
        $("#topup-custom").value = "";
      })
    );
    // 钱包页提示 → 跳转交易记录（明细统一入口）
    $("#wallet-goto-tx").addEventListener("click", () => switchView("transactions"));

    // 交易 Tab
    $$("#tx-tabs .tab").forEach((b) => b.addEventListener("click", () => { txTab = b.dataset.txTab; txTable.page = 1; renderTransactions(); }));

    // API Key 生成（行内编辑；列表展示脱敏、复制给完整 id）
    $("#new-api-key-btn").addEventListener("click", openNewKeyInline);
    $("#ak-new-ok").addEventListener("click", commitNewKey);
    $("#ak-new-cancel").addEventListener("click", closeNewKeyInline);
    $("#ak-new-name").addEventListener("keydown", (e) => {
      if (e.key === "Enter") commitNewKey();
      else if (e.key === "Escape") closeNewKeyInline();
    });

    // API Key 搜索 + 行内操作（复制 / 改名 / 删除[行内二次确认]）
    $("#ak-search").addEventListener("input", renderSettings);

    $("#api-keys").addEventListener("click", (e) => {
      const cp = e.target.closest("[data-key-copy]");
      if (cp) { copyKey(Number(cp.dataset.keyCopy)); return; }
      const rn = e.target.closest("[data-key-rename]");
      if (rn) { renameKey(Number(rn.dataset.keyRename)); return; }
      const dl = e.target.closest("[data-key-del]");
      if (dl) { confirmInline(dl, () => deleteKey(Number(dl.dataset.keyDel)), "确认删除？"); return; }
      if (e.target.closest("[data-new-key]")) openNewKeyInline();
    });

    // 管理台 Tabs
    $$("#admin-tabs .tab").forEach((b) => b.addEventListener("click", () => {
      $$("#admin-tabs .tab").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      renderAdmin();
    }));

    // 成员充值（管理台）：行内编辑（替代原生输入弹窗，Enter 确认 / Esc 取消）
    $("#emp-body").addEventListener("click", (e) => {
      const dd = e.target.closest("[data-emp-dept]");
      if (dd) { editEmpDept(Number(dd.dataset.empDept)); return; }
      const b = e.target.closest("[data-emp-topup]");
      if (!b) return;
      const emp = D.EMPLOYEES[Number(b.dataset.empTopup)];
      if (!emp) return;
      const row = b.closest("tr");
      if (!row) return;
      const cell = row.children[5];
      inlineForm(cell, {
        value: "5000",
        placeholder: "点数（正整数）",
        type: "number",
        width: "120px",
        validate: (raw) => {
          const amt = Number(raw);
          return (!raw || !Number.isInteger(amt) || amt <= 0) ? "请输入正整数点数金额" : null;
        },
        onSubmit: (raw) => {
          const amt = Number(raw);
          emp.quota += amt;
          renderAdmin();
          toast("已给 " + emp.name + " 充值 " + D.fmt(amt) + " 点", "success");
        },
        onCancel: () => renderAdmin(),
      });
    });

    // 加额申请审批（US-20：批准 → 成员余额+申请点数；驳回 → 仅更新状态）
    $("#raise-requests").addEventListener("click", (e) => {
      const ap = e.target.closest("[data-raise-approve]");
      if (ap) { approveRaise(Number(ap.dataset.raiseApprove)); return; }
      const rj = e.target.closest("[data-raise-reject]");
      if (rj) rejectRaise(Number(rj.dataset.raiseReject));
    });

    // 组织管理：部门搜索 / 添加 / 编辑 / 删除（事件委托；添加/编辑用行内展开表单）
    $("#od-search").addEventListener("input", renderAdmin);

    $("#add-dept-btn").addEventListener("click", () => openDeptForm(null));
    $("#dept-confirm").addEventListener("click", (e) => withLoading(e.currentTarget, confirmDept));
    $("#dept-cancel").addEventListener("click", () => { $("#dept-form-card").hidden = true; });
    // 键盘可达（B.9）：部门表单 Enter 提交、Esc 收起
    $("#dept-form-name").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("#dept-confirm").click(); } });
    $("#dept-form-quota").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("#dept-confirm").click(); } });
    $("#dept-form-card").addEventListener("keydown", (e) => { if (e.key === "Escape") { $("#dept-form-card").hidden = true; } });

    $("#dept-body").addEventListener("click", (e) => {
      const ed = e.target.closest("[data-dept-edit]");
      if (ed) { openDeptForm(Number(ed.dataset.deptEdit)); return; }
      const dl = e.target.closest("[data-dept-del]");
      if (dl) { confirmInline(dl, () => deleteDept(Number(dl.dataset.deptDel)), "确认删除部门？"); return; }
      if (e.target.closest("[data-dept-clear-search]")) {
        $("#od-search").value = "";
        renderAdmin();
      }
    });
  }

  /* ---------------- 初始化 ---------------- */

  document.addEventListener("DOMContentLoaded", () => {
    renderNav();
    bindEvents();
    renderView("dashboard");
    $("#side-balance").textContent = D.fmt(D.USER.balance);

    // 全局快捷键（rant 16:57:17 D）：/ 聚焦市场搜索；数字 1-7 切换侧边栏视图；Esc 关闭行内新建 key
    document.addEventListener("keydown", (e) => {
      const t = e.target;
      const typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
      if (e.key === "Escape" && !$("#ak-new-inline").hidden) { closeNewKeyInline(); return; }
      if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "/") {
        e.preventDefault();
        $("#mk-search").focus();
        return;
      }
      if (e.key >= "1" && e.key <= "7") {
        const item = NAV_ORDER[Number(e.key) - 1];
        if (item) switchView(item.id);
      }
    });
  });
})();
