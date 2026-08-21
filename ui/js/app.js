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
  const T = window.t; // i18n（rant 2026-08-18T20:49:22）

  let activeView = "dashboard";
  let txTab = "all";
  let isGuest = false; // 游客模式（US-1：未登录可浏览市场）
  let pendingHashView = null; // URL hash 路由（rant 20:39:30 A）：刷新后登录时恢复上次视图
  let mkExpanded = null; // 市场行展开（rant 20:39:30 F）：当前展开的模型 id，null=全部收起；仅展开当前行

  // MRT 风格表格状态（页面级变量：切换页面不丢失排序/筛选/分页）
  const txTable = { sort: [], filters: {}, page: 1, pageSize: 10 };

  /* ---------------- 工具 ---------------- */

  // toast 队列堆叠（rant 20:39:30 D：最多同时 3 条，纵向堆叠，独立淡入淡出；分级样式保留 success/error/info）
  // 可交互 toast（rant 20:46:57 B：opts.action = { label, onClick } → 内嵌按钮 + 更长展示时长）
  const TOAST_MAX = 3;
  const TOAST_MS = 2600;      // 展示时长
  const TOAST_ACTION_MS = 6000; // 可交互 toast 展示时长（给用户留点击时间）
  const TOAST_OUT_MS = 200; // 淡出时长
  function toast(msg, type, opts) {
    const wrap = $("#toast-wrap");
    const el = document.createElement("div");
    el.className = "toast" + (type ? " " + type : "");
    const action = opts && opts.action;
    if (action) {
      el.innerHTML = esc(msg) + ' <button type="button" class="toast-action">' + esc(action.label) + "</button>";
      const btn = el.querySelector(".toast-action");
      if (btn) btn.addEventListener("click", () => action.onClick && action.onClick());
    } else {
      el.textContent = msg;
    }
    wrap.appendChild(el);
    // 超过上限：移除最旧的一条（不等待淡出，立即腾位）
    while (wrap.children.length > TOAST_MAX) {
      const old = wrap.children[0];
      if (old && old.parentNode) old.parentNode.removeChild(old);
    }
    // 独立生命周期：到时淡出 → 移除（各条互不影响）
    setTimeout(() => {
      el.classList.add("out");
      setTimeout(() => { if (el.parentNode) el.parentNode.removeChild(el); }, TOAST_OUT_MS);
    }, action ? TOAST_ACTION_MS : TOAST_MS);
  }

  // 复制 API Key 后引导（rant 20:46:57 B：跳到设置页接入端点卡片并高亮闪烁）
  function gotoEndpointCard() {
    switchView("settings");
    const card = $("#endpoint-card");
    if (!card) return;
    card.classList.remove("ep-flash");
    void card.offsetWidth; // 重启动画
    card.classList.add("ep-flash");
    setTimeout(() => card.classList.remove("ep-flash"), 1600);
    if (card.scrollIntoView) card.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  // 快捷键帮助面板（rant 20:39:30 E：行内卡片非 modal；? / Shift+/ 开合，Esc 或再按 ? 关闭）
  const HELP_KEYS = [
    ["/", "help.k1"],
    ["1–7", "help.k2"],
    ["Esc", "help.k3"],
    ["?", "help.k4"],
  ];
  function renderHelp() {
    $("#help-body").innerHTML = HELP_KEYS.map(([k, d]) =>
      '<div class="help-row"><span class="kbd">' + esc(k) + "</span><span class=\"help-desc\">" + esc(T(d)) + "</span></div>").join("");
    const theme = document.documentElement.dataset.theme === "light" ? T("help.theme.light") : T("help.theme.dark");
    $("#help-context").textContent = T("help.context", { view: T(VIEW_TITLE[activeView] || activeView), theme: theme });
  }
  function toggleHelp(force) {
    const panel = $("#help-panel");
    const open = force !== undefined ? force : panel.classList.contains("hidden");
    if (open) renderHelp();
    panel.classList.toggle("hidden", !open);
  }

  /* --- 表格密度（rant 20:46:57 C：舒适/紧凑两档，localStorage atp-density 记忆，全站 .table 生效） --- */
  function applyDensity(d) {
    const app = $("#app");
    app.classList.toggle("density-compact", d === "compact");
    try { localStorage.setItem("atp-density", d); } catch (e) { /* 隐私模式忽略 */ }
  }
  function getDensity() { try { return localStorage.getItem("atp-density") === "compact" ? "compact" : "comfortable"; } catch (e) { return "comfortable"; } }

  /* --- 首次引导 tour（rant 20:46:57 A：非 modal 浮层 + 目标 accent 高亮环；localStorage atp-tour-done 控制） --- */
  const TOUR_STEPS = [
    { view: "dashboard",  sel: "#dash-stats",    title: "tour.step1.title",     desc: "tour.step1.desc" },
    { view: "marketplace", sel: "#view-marketplace", title: "tour.step2.title", desc: "tour.step2.desc" },
    { view: "sharing",    sel: "#view-sharing",  title: "tour.step3.title",   desc: "tour.step3.desc" },
    { view: "settings",   sel: "#endpoint-card", title: "tour.step4.title", desc: "tour.step4.desc" },
  ];
  let tourStep = -1; // -1 = 未在引导中

  function isTourDone() { try { return localStorage.getItem("atp-tour-done") === "1"; } catch (e) { return true; } }
  function markTourDone() { try { localStorage.setItem("atp-tour-done", "1"); } catch (e) { /* 隐私模式忽略 */ } }

  function maybeStartTour() { if (!isTourDone()) startTour(); }

  function startTour() {
    tourStep = 0;
    renderTourStep();
  }

  function renderTourStep() {
    const step = TOUR_STEPS[tourStep];
    if (!step) return;
    // 引导中自动切到对应视图（不入历史，避免后退需多次）
    if (step.view !== activeView) switchView(step.view, { sync: false });
    const target = document.querySelector(step.sel);
    const r = target && target.getBoundingClientRect ? target.getBoundingClientRect() : null;
    $("#tour-overlay").classList.remove("hidden");
    $("#tour-ring").classList.remove("hidden");
    $("#tour-pop").classList.remove("hidden");
    const ring = $("#tour-ring");
    if (r && r.width) {
      ring.style.top = r.top + "px";
      ring.style.left = r.left + "px";
      ring.style.width = r.width + "px";
      ring.style.height = r.height + "px";
    } else { ring.classList.add("hidden"); }
    $("#tour-title").textContent = T(step.title);
    $("#tour-desc").textContent = T(step.desc);
    $("#tour-progress").textContent = (tourStep + 1) + " / " + TOUR_STEPS.length;
    const pop = $("#tour-pop");
    if (r && r.width) pop.style.top = Math.min(r.bottom + 12, (window.innerHeight || 800) - 220) + "px";
    else pop.style.top = "84px";
    pop.style.left = Math.max(12, Math.min(r ? r.left : 12, (window.innerWidth || 800) - 312)) + "px";
    const prev = document.querySelector('#tour-pop [data-tour-action="prev"]');
    if (prev) prev.disabled = tourStep === 0;
    const next = document.querySelector('#tour-pop [data-tour-action="next"]');
    if (next) next.textContent = tourStep === TOUR_STEPS.length - 1 ? T("tour.done") : T("tour.next");
  }

  function closeTour() {
    if (tourStep < 0) return;
    markTourDone();
    tourStep = -1;
    $("#tour-overlay").classList.add("hidden");
    $("#tour-ring").classList.add("hidden");
    $("#tour-pop").classList.add("hidden");
  }

  // 按钮 loading 态（rant 15:50:05 B.8：提交中转圈，模拟反馈后恢复）
  const SPINNER = '<span class="spin" aria-hidden="true"></span>';
  function withLoading(btn, fn, ms) {
    if (!btn || btn.dataset.loading) return;
    const orig = btn.innerHTML;
    btn.dataset.loading = "1";
    btn.disabled = true;
    btn.innerHTML = SPINNER + " " + T("common.loading");
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

  // 数字/点数变化轻微跳动（rant 18:06:09 E：充值/消费后余额跳动；prefers-reduced-motion 下由 CSS 禁用动画，功能不受影响）
  function bump(el) {
    if (!el) return;
    el.classList.remove("bump");
    void el.offsetWidth; // 强制 reflow 以重放动画
    el.classList.add("bump");
  }

  /* --- 搜索增强（rant 18:06:09 D：防抖 + <mark> 关键词高亮 + 清空 × 按钮） --- */

  // 用户输入作为正则关键词时先转义，避免误当正则语法（如 "C++"、"("）
  function escapeRegExp(s) {
    return String(s).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  // 关键词高亮：先 HTML 转义，再对查询词大小写不敏感包 <mark>；无关键词返回转义原样（重置后自动清除）
  function hl(s, q) {
    const t = esc(s);
    if (!q) return t;
    const eq = esc(q); // 与转义后的正文同构，避免 & < > 等字符错位
    if (!eq) return t;
    const re = new RegExp(escapeRegExp(eq), "gi");
    return t.replace(re, (m) => "<mark>" + m + "</mark>");
  }

  // 搜索框接线：输入防抖渲染（默认 ~150ms，避免每次按键整表重绘闪烁）+ 清空 × 按钮（有内容时显示，点击清空立即重绘）
  function wireSearch(input, render, ms) {
    if (!input) return;
    const delay = ms || 150;
    const box = input.closest(".search-box");
    const clear = box ? box.querySelector(".search-clear") : null;
    const syncClear = () => { if (clear) clear.hidden = !input.value; };
    let t = null;
    input.addEventListener("input", () => {
      clearTimeout(t);
      t = setTimeout(() => render(), delay);
      syncClear();
    });
    if (clear) {
      clear.addEventListener("click", () => {
        input.value = "";
        syncClear();
        render(); // 清空立即重绘，不走防抖
        input.focus();
      });
    }
    syncClear();
  }

  // 程序化清空搜索框（值 + × 按钮态同步，不触发渲染；调用方随后自行重绘）
  function resetSearch(input) {
    if (!input) return;
    input.value = "";
    const box = input.closest(".search-box");
    const clear = box ? box.querySelector(".search-clear") : null;
    if (clear) clear.hidden = true;
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
    btn.innerHTML = confirmText || T("common.confirmInline");
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
    ok.type = "button"; ok.className = "btn btn-primary"; ok.textContent = T("common.confirm");
    ok.style.cssText = "padding:4px 10px;font-size:12px";
    const cancel = document.createElement("button");
    cancel.type = "button"; cancel.className = "btn btn-ghost"; cancel.textContent = T("common.cancel");
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
    const l = labels[status];
    const text = l && typeof l.text === "function" ? l.text() : (l ? l.text : status);
    return '<span class="badge ' + (l ? l.cls : "dim") + '">' + esc(text) + "</span>";
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
  // 零 mock（rant 2026-08-19T15:54:06）：登录态优先用 /api/models 真实价格（同 modelsToView 折算，
  // 锚定 CNY × points_per_unit=1；USD 价 ×7.2）；仅游客/表单兜底读 data.js
  function autoPrice(model) {
    const live = Live.models && Live.models.find((x) => x.model === model);
    if (live) {
      const mult = live.currency === "CNY" ? 1 : 7.2;
      return Math.round(live.output_per_m * mult * 1e5) / 1e5;
    }
    const m = D.MODELS.find((x) => x.model === model);
    if (m && typeof m.out === "number") return m.out;
    // 兜底：取同厂商相近模型的输出价；仍无则用固定默认价
    const same = m ? D.MODELS.find((x) => x.provider === m.provider && typeof x.out === "number") : null;
    return same ? same.out : 300;
  }

  // 厂商展示名：zh 用中文标签（阿里云百炼…），en 用 provider id（English-friendly）
  const provLabel = (p) => (I18n.lang === "zh" ? (D.PROVIDER_LABELS[p] || p) : p);

  // key 脱敏展示：仅显示前 3 后 4（如 sk-****1234）
  function maskKey(key) {
    if (!key) return "—";
    if (key.length <= 8) return key.slice(0, 3) + "****" + key.slice(-4);
    return key.slice(0, 3) + "****" + key.slice(-4);
  }

  function showPriceHint(model) {
    const el = $("#sf-price-view");
    if (!el) return;
    if (!model) { el.textContent = T("share.form.priceAuto"); return; }
    // 零 mock：登录态价格来自 /api/models（autoPrice 内部优先），data.js 仅表单兜底
    const known = (Live.models && Live.models.some((x) => x.model === model)) || D.MODELS.some((x) => x.model === model);
    if (known) {
      el.textContent = T("share.price.auto", { n: D.fmt(autoPrice(model)) });
    } else {
      el.textContent = T("share.price.default", { n: D.fmt(autoPrice(model)) });
    }
  }

  // Plan 提示：按量/订阅 + 专属端点说明（来自 PLANS；登录后为 /api/plans）
  function showPlanHint(planId) {
    const el = $("#sf-plan-hint");
    if (!el) return;
    const plans = Live.plans || D.PLANS;
    const pl = plans.find((x) => x.id === planId);
    if (!pl) { el.textContent = ""; return; }
    el.textContent = (pl.type === "paygo" ? T("share.plan.paygo") : T("share.plan.sub")) +
      (pl.note ? T("share.plan.note", { note: pl.note }) : "");
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
    { g: "nav.main", items: [
      { id: "dashboard", icon: "dashboard", label: "nav.dashboard" },
      { id: "marketplace", icon: "marketplace", label: "nav.marketplace" },
      { id: "sharing", icon: "sharing", label: "nav.sharing" },
      { id: "wallet", icon: "wallet", label: "nav.wallet" },
      { id: "transactions", icon: "transactions", label: "nav.transactions" },
    ]},
    { g: "nav.role", items: [
      { id: "admin", icon: "admin", label: "nav.admin", role: "admin" },
      { id: "ops", icon: "admin", label: "nav.ops", role: "ops" },
      { id: "settings", icon: "settings", label: "nav.settings" },
    ]},
  ];

  // 侧边栏视图顺序（rant 16:57:17 D：数字 1-7 切换对应视图，title 提示快捷键）
  const NAV_ORDER = NAV.flatMap((g) => g.items);

  const VIEW_TITLE = {
    dashboard: "view.dashboard", marketplace: "view.marketplace", sharing: "view.sharing",
    wallet: "view.wallet", transactions: "view.transactions", settings: "view.settings",
    admin: "view.admin",
    ops: "view.ops",
  };

  // 游客可见的页面（US-1：仅市场；其余需登录）
  const GUEST_VIEWS = ["marketplace"];

  const VALID_VIEWS = Object.keys(VIEW_TITLE);

  /* --- URL hash 路由（rant 20:39:30 A：刷新/前进后退保持视图；非法 hash 回仪表盘） --- */

  // 从 location.hash 解析视图 id：空 hash → null（不动作）；非法 hash → "dashboard"
  function viewFromHash() {
    const h = (window.location && window.location.hash) || "";
    if (!h || h === "#") return null;
    const m = h.match(/^#\/([\w-]+)/);
    const id = m ? m[1] : null;
    return VALID_VIEWS.includes(id) ? id : "dashboard";
  }

  // 当前 hash 是否为合法视图 hash（非法 hash 回退仪表盘时避免重写 URL、污染历史）
  function hashIsValid() {
    const h = (window.location && window.location.hash) || "";
    const m = h.match(/^#\/([\w-]+)/);
    return !!m && VALID_VIEWS.includes(m[1]);
  }

  // 视图切换后同步 URL（pushState 不触发 hashchange，避免回环；已相同则跳过不重复入栈）
  function syncHash(id) {
    if (!window.history || !window.history.pushState) return;
    const h = "#/" + id;
    if ((window.location && window.location.hash) === h) return;
    try { window.history.pushState(null, "", h); } catch (e) { /* file:// 下个别浏览器限制，忽略 */ }
  }

  function renderNav() {
    const nav = $("#nav");
    nav.innerHTML = "";
    // P2-A：角色视图按当前用户 role 显隐（admin 项仅 role=admin 可见）
    const roleNav = NAV
      .map((g) => ({ g: g.g, items: g.items.filter((it) => !it.role || D.USER.role === it.role) }))
      .filter((g) => g.items.length > 0);
    const groups = isGuest
      ? [{ g: "nav.guest", items: [
          { id: "marketplace", icon: "marketplace", label: T("nav.marketplace") },
        ]}]
      : roleNav;
    groups.forEach((group) => {
      const g = document.createElement("div");
      g.className = "nav-group";
      g.textContent = T(group.g);
      nav.appendChild(g);
      group.items.forEach((item) => {
        const b = document.createElement("button");
        b.className = "nav-item" + (item.id === activeView ? " active" : "");
        b.dataset.view = item.id;
        const short = NAV_ORDER.indexOf(item) + 1; // 1-7
        b.title = T("nav.shortcut", { n: short, label: T(item.label) });
        b.innerHTML = '<span class="ico">' + (ICONS[item.icon] || "") + '</span><span class="label">' + esc(T(item.label)) + "</span>" +
          (item.role ? "" : '<span class="nav-key">' + short + "</span>");
        if (item.role) {
          const tag = document.createElement("span");
          tag.className = "nav-tag";
          tag.textContent = item.role === "ops" ? T("nav.tag.ops") : T("nav.tag.admin");
          b.appendChild(tag);
        }
        b.addEventListener("click", () => switchView(item.id));
        nav.appendChild(b);
      });
    });
    $("#mode-label").textContent = isGuest ? T("nav.mode.guest") : T("nav.mode.normal");
  }

  function switchView(id, opts) {
    // 游客限制（US-1）：非市场页面 → 提示需登录
    if (isGuest && !GUEST_VIEWS.includes(id)) {
      toast(T("view.guest.lock", { view: T(VIEW_TITLE[id] || id) }), "error");
      return;
    }
    // 角色限制（P2-A/P2-C）：管理视图仅 admin；运营视图仅 ops（hash 直达 / 快捷键也兜底）
    if (id === "admin" && D.USER.role !== "admin") {
      toast(T("view.admin.lock"), "error");
      return;
    }
    if (id === "ops" && D.USER.role !== "ops") {
      toast(T("view.ops.lock"), "error");
      return;
    }
    activeView = id;
    $$(".view").forEach((v) => v.classList.add("hidden"));
    $("#view-" + id).classList.remove("hidden");
    renderNav();
    renderView(id);
    $("#main").scrollTop = 0;
    // 动态文档标题（rant 18:06:09 F：视图切换跟随「视图 · AITokenPool」，未知视图回默认）
    document.title = VIEW_TITLE[id] ? T(VIEW_TITLE[id]) + " · AITokenPool" : "AITokenPool";
    // URL hash 路由（rant 20:39:30 A：视图切换同步 #/视图；非法 hash 回退时不清 URL，避免污染历史）
    if (!opts || opts.sync !== false) syncHash(id);
  }

  /* ---------------- 视图渲染 ---------------- */

  function renderView(id) {
    // P2-B：先渲染（缓存/mock），再异步拉真实数据刷新（登录时）
    if (id === "dashboard") { renderDashboard(); if (loggedIn()) loadDashboard(); }
    else if (id === "marketplace") { renderMarketplace(); if (loggedIn()) loadMarketplace(); }
    else if (id === "sharing") { renderSharing(); if (loggedIn()) loadSharing(); }
    else if (id === "wallet") renderWallet();
    else if (id === "transactions") { renderTransactions(); if (loggedIn()) loadTransactions(); }
    else if (id === "settings") { renderSettings(); if (loggedIn()) loadApiKeys(); }
    else if (id === "admin") { renderAdmin(); if (loggedIn() && D.USER.role === "admin") loadAdmin(); }
    else if (id === "ops") { renderOps(); if (loggedIn() && D.USER.role === "ops") loadOps(); }
  }

  /* --- 仪表盘 --- */

  function renderDashboard() {
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不读取 D.TRANSACTIONS ——
    // 登录 → /api/wallet（month_use/month_earn）+ /api/dashboard；未就绪显示 0（随后异步刷新）
    let monthUse = 0, monthEarn = 0, tradeCount = 0;
    if (loggedIn()) {
      if (Live.wallet) {
        monthUse = Live.wallet.month_use || 0;
        monthEarn = Live.wallet.month_earn || 0;
      }
      tradeCount = Live.transactions ? (Live.transactions.total || 0) : 0;
    } else {
      const txs = D.TRANSACTIONS || [];
      monthUse = txs.filter((t) => t.type === "consume").reduce((a, t) => a + Math.abs(t.pts), 0);
      monthEarn = txs.filter((t) => t.type === "earn").reduce((a, t) => a + t.pts, 0);
      tradeCount = txs.length;
    }

    $("#dash-stats").innerHTML = [
      stat(T("dash.balance"), D.fmt(D.USER.balance), "", "accent"),
      stat(T("dash.usage"), D.fmt(monthUse) + " " + T("common.points"), T("dash.usage.sub")),
      stat(T("dash.earnings"), "+" + D.fmt(monthEarn) + " " + T("common.points"), T("dash.earnings.sub")),
      stat(T("dash.trades"), T("cnt.trades", { n: tradeCount }), T("dash.trades.sub")),
    ].join("");

    // 降级原则（rant 2026-08-19T15:48:17 / 15:54:06）：mock 只用于游客模式；
    // 登录态加载失败 → 空态 + 重试（loadErrorHtml），不静默 fallback 到 D.SHARINGS
    const shares = Live.sharings ? sharingsToView(Live.sharings) : (loggedIn() ? null : (D.SHARINGS || []));
    if (shares) {
      const on = shares.filter((s) => s.status === "on");
      $("#dash-sharings").innerHTML = on.map((s) =>
        '<div class="mini-item"><div><div class="t">' + esc(s.model) + "</div>" +
        '<div class="d">' + esc(s.plan || "API") + " · " + T("dash.used", { used: D.fmt(s.used), quota: D.fmt(s.quota), price: D.fmt(s.price) }) + "</div></div>" +
        '<div class="r"><span class="pts">+' + D.fmt(s.earned) + "</span><div class='d'>" + T("dash.earned") + "</div></div></div>"
      ).join("") + (on.length ? "" : '<div class="empty-state compact">' + EMPTY_ICON + "<p>" + T("dash.noSharing") + "</p><p class='muted'>" + T("dash.noSharing.sub") + "</p></div>");
      // 共享收益累计趋势 sparkline（rant 18:06:09 A；无上架 key 时保留空状态，不画图）
      // 零 mock：仅游客用 D.TRANSACTIONS 演示；登录态后端无按日 earn 序列 → 不画假趋势
      if (on.length && !loggedIn()) {
        const days = lastDayLabels(7);
        const earn = dailySeries(days, (t) => t.type === "earn");
        let cum = 0;
        const cumSeries = earn.map((v) => { cum = Math.round((cum + v) * 1e5) / 1e5; return cum; });
        $("#dash-sharings").insertAdjacentHTML("afterbegin",
          sparkline(cumSeries, { labels: days, fmt: (v) => "+" + D.fmt(v), stroke: "var(--ok)" }));
      }
    } else {
      $("#dash-sharings").innerHTML = loadErrorHtml(T("dash.loadFail"), () => loadDashboard(), T("err.loadFail"));
    }
    renderMonthChanges();
  }

  // P2-B：拉取仪表盘所需数据（wallet + dashboard + sharings + 交易数）
  async function loadDashboard() {
    if (!loggedIn()) return;
    try { await refreshWallet(); } catch (e) { /* 降级 */ }
    try {
      Live.dashboard = await api.get("/api/dashboard");
    } catch (e) { Live.dashboard = null; }
    // 交易数统计（dash.trades）：拉 1 条取 total（零 mock，rant 2026-08-19T15:54:06）
    try {
      Live.transactions = await api.get("/api/transactions?page=1&page_size=1");
    } catch (e) { Live.transactions = null; }
    // rant 2026-08-19T15:48:17：仪表盘「我的共享」此前从不拉 sharings → 登录态恒显示 D.SHARINGS mock；
    // 现在拉真实数据；失败置 null → renderDashboard 走空态 + 重试（mock 仅游客）
    try {
      Live.sharings = await api.get("/api/sharings");
    } catch (e) { Live.sharings = null; }
    renderDashboard();
  }

  function stat(label, value, sub, cls) {
    return '<div class="stat' + (cls ? " " + cls : "") + '"><div class="label">' + esc(label) +
      '</div><div class="value">' + value + "</div><div class='sub'>" + esc(sub) + "</div></div>";
  }

  /* --- 模型市场 --- */

  // 最近使用（rant 20:46:57 D：localStorage atp-recent-models 最近 5 个去重，复用 .chip，点击直接使用）
  const RECENT_MAX = 5;
  const RECENT_KEY = "atp-recent-models";
  function getRecentIds() {
    try {
      const arr = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
      return Array.isArray(arr) ? arr.filter((x) => Number.isFinite(+x)).map(Number) : [];
    } catch (e) { return []; } // 旧数据/隐私模式：按空处理
  }
  function saveRecentIds(ids) {
    try { localStorage.setItem(RECENT_KEY, JSON.stringify(ids.slice(0, RECENT_MAX))); } catch (e) { /* 隐私模式忽略 */ }
  }
  function markRecentUsed(id) {
    const ids = getRecentIds().filter((x) => x !== id); // 去重：已存在则先移除
    ids.unshift(id);                                     // 最新使用放最前
    saveRecentIds(ids);
  }
  function renderRecent() {
    const wrap = $("#mk-recent-chips");
    const chips = getRecentIds().map((id) => {
      const m = (Live.models ? modelsToView(Live.models) : D.MARKET).find((x) => x.id === id);
      return m ? '<button type="button" class="chip" data-recent-model="' + id + '" title="' + esc(m.provider) + " · " + T("mk.recent.use") + '">' + esc(m.model) + "</button>" : null;
    }).filter(Boolean);
    wrap.innerHTML = chips.join("");
    $("#mk-recent").hidden = chips.length === 0;
  }

  // 市场行展开详情（rant 20:39:30 F：max tokens / 1M tokens ≈ N 点换算 / 多 key 故障转移说明）
  function mkDetailHtml(m) {
    // 零 mock（rant 2026-08-19T15:54:06）：live 行 max/success 后端暂无 → 显示「—」；
    // 仅游客行可查 data.js MODELS（mock 仅游客）
    const md = (!m.live) ? D.MODELS.find((x) => x.model === m.model) : null;
    const maxTok = (md && md.max) ? D.fmt(md.max) : T("mk.detail.unpublished");
    const succ = m.success == null ? "—" : m.success;
    const items = [
      [T("mk.detail.max"), maxTok],
      [T("mk.detail.price"), T("mk.detail.priceVal", { out: D.fmt(m.out), in: D.fmt(m.in) })],
      [T("mk.detail.ctx"), T("mk.detail.ctxVal", { n: D.ctxFmt(m.ctx) })],
      [T("mk.detail.avail"), m.avail ? T("mk.detail.availOn", { p: succ }) : T("mk.detail.availOff", { p: succ })],
    ];
    // 高峰时段价（rant 2026-08-20T11:58:40：DeepSeek 高峰 9-12/14-18 北京时翻倍）
    if (m.peak) items.push([T("mk.detail.peak"), T("mk.detail.peakVal", { out: D.fmt(m.peakOut), in: D.fmt(m.peakIn) })]);
    if (m.multi) items.push([T("mk.detail.route"), T("mk.detail.routeVal")]);
    return '<div class="mk-detail-grid">' + items.map(([k, v]) =>
      '<div class="mkd-item"><span class="mkd-label">' + esc(k) + '</span><span class="mkd-val">' + esc(v) + "</span></div>").join("") + "</div>";
  }

  function renderMarketplace() {
    const rawQ = $("#mk-search").value || "";
    const q = rawQ.toLowerCase();
    const prov = $("#mk-provider").value;
    const sort = $("#mk-sort").value;

    // 厂商筛选下拉：登录态用 /api/models 真实厂商；游客用 data.js（零 mock，rant 15:54:06）
    const provEl = $("#mk-provider");
    if (provEl && provEl.dataset.provSource !== (Live.models ? "live" : "mock")) {
      const providers = Live.models ? [...new Set(Live.models.map((m) => m.provider))] : D.PROVIDERS;
      provEl.dataset.provSource = Live.models ? "live" : "mock";
      const cur = provEl.value;
      provEl.innerHTML = '<option value="">' + T("mk.provider.all") + "</option>" +
        providers.map((p) => '<option value="' + p + '">' + p + "</option>").join("");
      if (cur && providers.includes(cur)) provEl.value = cur;
    }

    // P2-B：登录 → /api/models 真实列表；游客 → data.js mock（mock 仅游客，rant 15:54:06）；
    // 登录态加载失败 → 空态 + 重试（loadErrorRow），绝不 fallback D.MARKET
    let list = Live.models ? modelsToView(Live.models) : (loggedIn() ? null : D.MARKET);
    if (!list) {
      $("#mk-count").textContent = T("cnt.on", { n: 0 });
      $("#mk-body").innerHTML = loadErrorRow(7, T("mk.loadFail"), T("err.loadFail"));
      pulseTbody($("#mk-body"));
      renderRecent();
      return;
    }
    list = list.filter((m) =>
      (!q || m.model.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q)) &&
      (!prov || m.provider === prov)
    );
    if (sort === "price-asc") list = [...list].sort((a, b) => a.in - b.in);
    else if (sort === "price-desc") list = [...list].sort((a, b) => b.in - a.in);
    else if (sort === "ctx-desc") list = [...list].sort((a, b) => b.ctx - a.ctx);

    // 游客提示（P2-B：游客浏览用静态列表 + 顶部提示）
    const guestHint = isGuest
      ? '<div class="empty-state compact" style="margin-bottom:10px">' + EMPTY_ICON +
        "<p>" + T("mk.guest.hint") + "</p><p class='muted'>" + T("mk.guest.hint.sub") + "</p></div>"
      : "";

    $("#mk-count").textContent = T("cnt.on", { n: list.length });
    $("#mk-body").innerHTML = guestHint + (list.length ? list.map((m) =>
      "<tr><td data-label='厂商'>" +
      '<button type="button" class="row-expand" data-mk-expand="' + m.id + '" title="' + (mkExpanded === m.id ? T("mk.collapse") : T("mk.expand")) + '">' + (mkExpanded === m.id ? "−" : "+") + "</button>" +
      hl(m.provider, rawQ) + "</td><td data-label='模型'><strong>" + hl(m.model, rawQ) + "</strong></td>" +
      '<td class="num" data-label="输入价 /1M">' + D.fmt(m.in) + " " + T("common.points") +
      (m.peak ? ' <span class="badge warn" title="' + esc(T("mk.peak.title", { n: m.peakMult })) + '">' + esc(T("mk.peak.badge", { n: m.peakMult })) + "</span>" : "") + "</td>" +
      '<td class="num" data-label="输出价 /1M">' + D.fmt(m.out) + " " + T("common.points") + "</td>" +
      '<td class="num" data-label="上下文">' + D.ctxFmt(m.ctx) + "</td>" +
      "<td data-label='可用性'>" + (m.avail ? '<span class="badge ok">' + T("mk.avail") + "</span>" : '<span class="badge warn">' + T("mk.busy") + "</span>") +
      (m.multi ? ' <span class="badge ok" title="' + T("mk.multi") + '">' + T("mk.multi") + "</span>" : "") + "</td>" +
      "<td data-label='操作'><button class='btn btn-primary' style='padding:4px 10px;font-size:12px' data-use-model='" + m.id + "'" + (m.avail ? "" : " disabled") + ">" + T("mk.use") + "</button>" +
      // 零 mock：成功率后端暂无字段 → 仅当有真实值时展示（multi/success 已从 data.js 移除）
      (m.success != null ? "<div class='muted' style='margin-top:4px;font-size:12px'>" + T("mk.success", { p: m.success }) + "</div>" : "") + "</td></tr>" +
      (mkExpanded === m.id ? '<tr class="mk-detail"><td colspan="7">' + mkDetailHtml(m) + "</td></tr>" : "")
    ).join("") : emptyRow(7, T("mk.empty"), T("mk.empty.sub"),
      '<button type="button" class="btn btn-ghost" data-mk-clear-filters>' + T("mk.clearFilters") + "</button>"));
    pulseTbody($("#mk-body"));
    renderRecent(); // 最近使用 chips（rant 20:46:57 D）
  }

  // P2-B：拉取市场真实模型（登录时）；失败 → 空态 + 重试（不 mock，rant 15:54:06）
  async function loadMarketplace() {
    if (!loggedIn()) return;
    try {
      await liveLoad("models", "/api/models");
    } catch (e) { Live.models = null; /* 登录态降级空态 */ }
    renderMarketplace();
  }

  /* --- 可用时间段（rant 10:54:48：结构化字段，备注只作纯备注） --- */

  const DAY_LABELS = [1, 2, 3, 4, 5, 6, 7];

  // 星期数字 → 展示文本：连续区间压缩为「周一~周五」，间断用 / 连接
  function fmtDays(nums) {
    const sorted = [...nums].sort((a, b) => a - b);
    const parts = [];
    let i = 0;
    while (i < sorted.length) {
      let j = i;
      while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++;
      parts.push(sorted[i] === sorted[j]
        ? T("share.day." + sorted[i])
        : T("share.day." + sorted[i]) + "~" + T("share.day." + sorted[j]));
      i = j + 1;
    }
    return parts.join("/");
  }

  function fmtAvailable(s) {
    const a = s && s.available;
    if (!a || !a.days || !a.days.length) return T("share.allDay");
    const t = a.start && a.end ? " " + a.start + "-" + a.end : "";
    return fmtDays(a.days) + t;
  }

  /* --- 共享管理 --- */

  const SHARE_STATUS = {
    on: { text: () => T("share.status.on"), cls: "ok" },
    paused: { text: () => T("share.status.paused"), cls: "warn" },
    off: { text: () => T("share.status.off"), cls: "dim" },
  };

  function renderSharing() {
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不 fallback D.SHARINGS；
    // 加载失败 → 空态 + 重试（loadErrorRow，tbody 内合法）
    const list = Live.sharings ? sharingsToView(Live.sharings) : null;
    if (!list) {
      $("#share-stats").innerHTML = "";
      $("#share-body").innerHTML = loadErrorRow(8, T("share.loadFail"), T("err.loadFail"));
      return;
    }
    const on = list.filter((s) => s.status === "on");
    const totalEarned = list.reduce((a, s) => a + s.earned, 0);
    const totalUsed = list.reduce((a, s) => a + s.used, 0);

    $("#share-stats").innerHTML = [
      stat(T("share.stats.listings"), T("cnt.keys", { n: on.length }), T("cnt.hist", { n: list.length })),
      stat(T("share.stats.earnings"), "+" + D.fmt(totalEarned) + " " + T("common.points"), T("share.stats.earnings.sub")),
      stat(T("share.stats.used"), D.fmt(totalUsed) + " " + T("common.points"), T("cnt.quota", { n: D.fmt(list.reduce((a, s) => a + s.quota, 0)) })),
    ].join("");

    // 表单下拉（厂商 → Plan → 模型 三级联动；Plan 中「API」= 按量计价的 key）
    const selP = $("#sf-provider");
    if (!selP.dataset.init) {
      // Bug 1 修复：优先用 /api/plans（后端 config [[plans]]），未登录/失败降级 data.js 对齐清单
      const plans = Live.plans || D.PLANS;
      const planProviders = [...new Set(plans.map((pl) => pl.provider))];
      selP.innerHTML = '<option value="">' + T("share.select.provider") + "</option>" + planProviders
        .map((p) => '<option value="' + p + '">' + esc(provLabel(p)) + "</option>").join("");
      const selPlan = $("#sf-plan");
      const selM = $("#sf-model");
      const fillModels = () => {
        const plan = plans.find((pl) => pl.id === selPlan.value);
        const p = plan ? plan.provider : selP.value;
        // 零 mock（rant 15:54:06）：模型下拉登录态用 /api/models（Live.models），游客/兜底 data.js
        const modelSrc = Live.models ? Live.models : D.MODELS;
        selM.innerHTML = '<option value="">' + T("share.select.model") + "</option>" + modelSrc.filter((m) => !p || m.provider === p)
          .map((m) => '<option value="' + m.model + '">' + m.model + "</option>").join("");
        showPriceHint(selM.value);
      };
      const fillPlans = () => {
        const p = selP.value;
        selPlan.innerHTML = '<option value="">' + T("share.select.plan") + "</option>" + plans.filter((pl) => pl.provider === p)
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

    $("#share-body").innerHTML = list.length ? list.map((s, i) =>
      "<tr><td data-label='厂商 · Plan / 模型'><strong>" + esc(provLabel(s.provider)) + " · " + esc(s.plan || "API") +
      "</strong><div class='muted' style='font-size:12px'>" + esc(s.model) + " · " + esc(fmtAvailable(s)) + "</div></td>" +
      "<td data-label='Key' class='mono'>" + esc(maskKey(s.key)) + "</td>" +
      "<td data-label='已用/额度' class='num'>" + D.fmt(s.used) + " / " + D.fmt(s.quota) + "</td>" +
      '<td class="num" data-label="单价">' + D.fmt(s.price) + " " + T("share.priceUnit") + "</td>" +
      '<td class="num" data-label="收益">+' + D.fmt(s.earned) + " " + T("common.points") + "</td>" +
      "<td data-label='上架时间'>" + timeCell(s.time) + "</td>" +
      "<td data-label='状态'>" + badge(s.status, SHARE_STATUS) + "</td>" +
      "<td data-label='操作'><button class='btn btn-ghost' data-share-toggle='" + i + "' style='padding:4px 10px;font-size:12px'>" +
      (s.status === "on" ? T("share.toggle.pause") : s.status === "paused" ? T("share.toggle.resume") : T("share.toggle.relist")) + "</button> " +
      "<button class='btn btn-danger' data-share-delete='" + i + "' style='padding:4px 10px;font-size:12px'>" + T("common.delete") + "</button></td></tr>"
    ).join("") : emptyRow(8, T("share.empty"), T("share.empty.sub"),
      '<button type="button" class="btn btn-primary" data-share-add>' + T("share.empty.add") + "</button>");
    pulseTbody($("#share-body"));
  }

  // P2-B：拉取我的共享（登录时）
  async function loadSharing() {
    if (!loggedIn()) return;
    try {
      await liveLoad("sharings", "/api/sharings");
    } catch (e) { Live.sharings = null; /* 登录态降级空态 */ }
    try {
      // Bug 1 修复：上架表单 Plan 数据源改真实后端（config [[plans]] 单一真源）
      await liveLoad("plans", "/api/plans");
    } catch (e) { Live.plans = null; /* 表单兜底 data.js 对齐清单 */ }
    try {
      // 零 mock（rant 15:54:06）：上架表单模型下拉 + 定价用真实模型表
      await liveLoad("models", "/api/models");
    } catch (e) { Live.models = null; /* 表单兜底 data.js */ }
    renderSharing();
  }

  async function deleteSharing(i) {
    if (!Live.sharings) return;
    const s = sharingsToView(Live.sharings)[i];
    if (!s || !s.id) return;
    try {
      await api.patch("/api/sharings/" + s.id, { status: "off" }); // 软删
      await loadSharing();
      toast(T("share.del.ok", { model: s.model }), "success");
    } catch (err) {
      toast((err && err.message) ? I18n.mapErr(err.message) : T("share.del.fail"), "error");
    }
  }

  async function toggleSharing(i) {
    if (!Live.sharings) return;
    const s = sharingsToView(Live.sharings)[i];
    if (!s || !s.id) return;
    const next = s.status === "on" ? "paused" : "on";
    try {
      await api.patch("/api/sharings/" + s.id, { status: next });
      await loadSharing();
      toast(next === "paused" ? T("share.toggle.paused", { model: s.model }) : T("share.toggle.resumed", { model: s.model }), "success");
    } catch (err) {
      toast((err && err.message) ? I18n.mapErr(err.message) : T("share.op.fail"), "error");
    }
  }

  /* --- 本月点数变化（rant 10:45:27：近 1 月按类型汇总收支，取代静态"点数来源"分组） --- */

  const MONTH_TYPE_LABELS = [
    ["gift", () => T("tx.type.gift")],
    ["expire", () => T("tx.type.expire")],
    ["earn", () => T("tx.type.earn")],
    ["consume", () => T("tx.type.consume")],
    ["topup", () => T("tx.type.topup")],
    ["withdraw", () => T("tx.type.withdraw")],
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
  // 仅游客演示用（rant 15:54:06：登录态不读取 D.TRANSACTIONS）
  function dailySeries(days, filter) {
    const map = {};
    (D.TRANSACTIONS || []).forEach((t) => {
      if (filter && !filter(t)) return;
      const day = String(t.time || "").slice(0, 5);
      map[day] = (map[day] || 0) + t.pts;
    });
    return days.map((d) => Math.round((map[d] || 0) * 1e5) / 1e5);
  }

  // UTC 日期串（'YYYY-MM-DDT00:00:00Z'）→ 本地 MM-DD（rant 2026-08-19T20:45:32 跨天不错位）
  function localMD(dateStr) {
    const p2 = (x) => String(x).padStart(2, "0");
    const d = new Date(String(dateStr).includes("T") ? dateStr : String(dateStr).replace(" ", "T") + "Z");
    if (isNaN(d.getTime())) return String(dateStr || "").slice(5);
    return p2(d.getMonth() + 1) + "-" + p2(d.getDate());
  }

  function renderMonthChanges() {
    let rowsHtml = "";
    let net = 0;
    let sparkData = null;
    let sparkLabels = null;
    if (Live.dashboard) {
      // P2-B：/api/dashboard month 聚合 + series
      const sums = {};
      (Live.dashboard.month || []).forEach((m) => { sums[m.type] = (sums[m.type] || 0) + (m.pts || 0); });
      net = Live.dashboard.net || 0;
      rowsHtml = MONTH_TYPE_LABELS
        .filter(([k]) => sums[k])
        .map(([k, label]) => monthChangeItem(label(), sums[k], false)).join("");
      const series = Live.dashboard.series || [];
      sparkData = series.map((s) => s.pts || 0);
      sparkLabels = series.map((s) => localMD(String(s.date || "")));
    } else if (!loggedIn()) {
      // 游客演示：data.js 内嵌交易聚合（mock 仅游客，rant 15:54:06）
      const txs = D.TRANSACTIONS || [];
      const sums = {};
      txs.forEach((t) => { sums[t.type] = (sums[t.type] || 0) + t.pts; });
      net = txs.reduce((a, t) => a + t.pts, 0);
      rowsHtml = MONTH_TYPE_LABELS
        .filter(([k]) => sums[k])
        .map(([k, label]) => monthChangeItem(label(), sums[k], false)).join("");
      const days = lastDayLabels(7);
      sparkData = dailySeries(days);
      sparkLabels = days;
    } else {
      // 登录态但 /api/dashboard 未就绪：净 0 + 空行（绝不读取 mock）
      net = 0;
      rowsHtml = "";
      sparkData = [];
      sparkLabels = [];
    }
    const html = monthChangeItem(T("dash.net"), net, true) +
      (rowsHtml ? rowsHtml : '<p class="muted">' + T("dash.noChange") + "</p>");
    const walletEl = $("#month-changes");
    if (walletEl) walletEl.innerHTML = html;
    const dashEl = $("#dash-month-changes");
    if (dashEl) {
      // 迷你折线图（rant 18:06:09 A：按天聚合净变化，hover 显示当天数值）
      dashEl.innerHTML = sparkline(sparkData, { labels: sparkLabels, fmt: (v) => (v > 0 ? "+" : "") + D.fmt(v) }) + html;
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
        setFieldError($("#topup-custom"), T("wallet.err.amount"));
        return;
      }
    }
    clearFieldError($("#topup-custom"));
    // 充值为模拟支付（演示；真实支付后续接入）——仅更新会话余额，不写 D.TRANSACTIONS（rant 15:54:06 已删）
    D.USER.balance = Math.round((D.USER.balance + amt) * 1e5) / 1e5;
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    renderWallet();
    bump($("#side-balance")); // 余额跳动（rant 18:06:09 E）
    bump($("#wallet-balance"));
    closeTopup();
    toast(T("wallet.topup.ok", { amt: D.fmt(amt) }), "success");
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
    if (!rawAmt || !Number.isInteger(amt) || amt <= 0) { setFieldError($("#raise-amount"), T("wallet.raise.err.amount")); firstErr = firstErr || $("#raise-amount"); }
    else clearFieldError($("#raise-amount"));
    if (!reason) { setFieldError($("#raise-reason"), T("wallet.raise.err.reason")); firstErr = firstErr || $("#raise-reason"); }
    else clearFieldError($("#raise-reason"));
    if (firstErr) { firstErr.focus(); return; }
    // 加额申请默认需管理员审批（原「需审批」开关随组织设置表单移除，见 rant 10:59:23）
    // P2-C：真实提交 POST /api/raise-requests（钱包页仅登录可达，零 mock rant 15:54:06）
    api.post("/api/raise-requests", { amount: amt, reason }).then(() => {
      closeRaise();
      toast(T("wallet.raise.ok", { amt: D.fmt(amt) }), "success");
    }).catch((err) => {
      const msg = (err && err.message) ? I18n.mapErr(err.message) : T("wallet.raise.fail");
      toast(msg, "error");
    });
  }

  /* --- 管理员：加额申请审批（US-20） --- */

  const RAISE_STATUS = {
    pending: { text: () => T("admin.raise.status.pending"), cls: "warn" },
    approved: { text: () => T("admin.raise.status.approved"), cls: "ok" },
    rejected: { text: () => T("admin.raise.status.rejected"), cls: "dim" },
  };

  function renderRaiseRequests() {
    const el = $("#raise-requests");
    if (!el) return;
    // 零 mock（rant 15:54:06）：登录态绝不 fallback D.RAISE_REQUESTS；失败 → 空态 + 重试
    const list = Live.raiseRequests;
    if (!list) {
      el.innerHTML = loadErrorHtml(T("admin.raise.loadFail"), null, T("err.loadFail"));
      return;
    }
    el.innerHTML = (list.length ? '<div class="table-wrap compact"><table class="table"><thead><tr><th>成员</th><th class="num">申请点数</th><th>原因</th><th>状态</th><th></th></tr></thead><tbody>' +
      list.map((r, i) =>
        "<tr><td data-label='成员'><strong>" + esc(r.name || r.user) + "</strong><div class='muted' style='font-size:12px'>" + esc(r.email) + "</div></td>" +
        '<td class="num" data-label="申请点数">+' + D.fmt(r.amount) + " " + T("common.points") + "</td>" +
        "<td data-label='原因'>" + esc(r.reason) + "</td>" +
        "<td data-label='状态'>" + badge(r.status, RAISE_STATUS) + "</td>" +
        "<td data-label='操作'>" + (r.status === "pending"
          ? "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-raise-approve='" + i + "'>" + T("admin.raise.approve") + "</button> " +
            "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-raise-reject='" + i + "'>" + T("admin.raise.reject") + "</button>"
          : '<span class="muted" style="font-size:12px">' + esc((r.created_at || "").slice(5, 16)) + "</span>") + "</td></tr>"
      ).join("") + "</tbody></table></div>"
      : emptyState(T("admin.raise.empty"), T("admin.raise.empty.sub")));
  }

  function approveRaise(i) {
    const list = Live.raiseRequests || [];
    const r = list[i];
    if (!r || r.status !== "pending") return;
    api.post("/api/admin/raise-requests/" + r.id + "/approve", {}).then(async () => {
      await loadAdmin();
      toast(T("admin.raise.approve.ok", { name: r.name || r.email, amt: D.fmt(r.amount) }), "success");
    }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.raise.approve.fail"), "error"));
  }

  function rejectRaise(i) {
    const list = Live.raiseRequests || [];
    const r = list[i];
    if (!r || r.status !== "pending") return;
    api.post("/api/admin/raise-requests/" + r.id + "/reject", {}).then(async () => {
      await loadAdmin();
      toast(T("admin.raise.reject.ok", { name: r.name || r.email }), "success");
    }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.raise.reject.fail"), "error"));
  }

  /* --- 交易记录 --- */

  const TX_TYPE = {
    consume: () => T("tx.type.consume"), earn: () => T("tx.type.earn"), topup: () => T("tx.type.topup"),
    withdraw: () => T("tx.type.withdraw"), gift: () => T("tx.type.gift"),
  };
  const txType = (k) => (TX_TYPE[k] ? TX_TYPE[k]() : k);
  const txStatus = (s) => s === "成功" ? T("tx.status.success") : s === "处理中" ? T("tx.status.pending") : s === "入账" ? T("tx.status.credited") : s;

  const TX_COLUMNS = [
    { key: "time", title: () => T("tx.col.time"), sort: "string", filter: "text", render: (t) => timeCell(t.time) },
    { key: "type", title: () => T("tx.col.type"), sort: "string", filter: "select",
      options: () => ["consume", "earn", "topup", "withdraw", "gift"].map(txType),
      filterVal: (t) => txType(t.type),
      render: (t) => t.type === "earn" ? '<span class="badge ok">' + T("tx.type.earn") + "</span>" : t.type === "consume" ? '<span class="badge accent">' + T("tx.type.consume") + "</span>" : t.type === "gift" ? '<span class="badge ok">' + T("tx.type.gift") + "</span>" : '<span class="badge dim">' + esc(txType(t.type)) + "</span>" },
    { key: "partner", title: () => T("tx.col.partner"), sort: "string", filter: "text" },
    { key: "tokens", title: () => T("tx.col.tokens"), sort: "string", filter: "text", align: "num",
      render: (t) => t.tokenDetail
        ? '<div>' + t.tokens + '</div><div class="tx-brk" title="' + esc(T("tx.brk.title")) + '">' + t.tokenDetail + "</div>"
        : t.tokens },
    { key: "pts", title: () => T("tx.col.pts"), sort: "number", filter: "number-range", align: "num",
      render: (t) => '<span style="color:' + (t.pts > 0 ? "var(--ok)" : "var(--text)") + ';font-weight:600">' + (t.pts > 0 ? "+" : "") + D.fmt(t.pts) + "</span>" },
    { key: "status", title: () => T("tx.col.status"), sort: "string", filter: "select",
      options: () => ["成功", "入账", "处理中"].map(txStatus),
      filterVal: (t) => txStatus(t.status),
      render: (t) => t.status === "处理中" ? '<span class="badge warn">' + esc(txStatus(t.status)) + "</span>" : esc(txStatus(t.status)) },
  ];

  // 交易汇总条（rant 20:39:30 B + 00:04:21 + 00:07:08：改用后端 summary 全量 SQL 聚合，
  // 不再对当前页本地加总；口径 = income 白名单（earn/topup/gift）为正、consume 为负；
  // 附带 Token 统计 总/输入/缓存/输出，M 单位）
  function renderTxSummary(list) {
    const s = (Live.transactions && Live.transactions.summary) ? Live.transactions.summary : null;
    let income = 0, expense = 0;
    if (s) {
      income = s.income_pts || 0;
      expense = s.expense_pts || 0;
    } else {
      // 兜底（无 summary 的旧数据/游客 mock）：按 type 而非 pts 符号（rant 00:04:21 Bug A）
      list.forEach((t) => {
        if (t.type === "consume") expense += Math.abs(t.pts);
        else income += Math.abs(t.pts);
      });
    }
    const net = income - expense;
    const cls = (n) => (n > 0 ? "ok" : n < 0 ? "danger" : "");
    const fmt = (n) => (n > 0 ? "+" : n < 0 ? "-" : "") + D.fmt(Math.abs(n));
    const fmtM = (n) => (n >= 1e6 ? (n / 1e6).toFixed(2) + "M" : (n >= 1000 ? Math.round(n / 1000) + "K" : String(Math.round(n))));
    let html =
      '<div class="ts-item"><span class="ts-label">' + T("tx.summary.income") + "</span><span class='ts-value num ' + cls(income) + '\'>" + fmt(income) + "</span></div>" +
      '<div class="ts-item"><span class="ts-label">' + T("tx.summary.expense") + "</span><span class='ts-value num ' + cls(-expense) + '\'>" + fmt(-expense) + "</span></div>" +
      '<div class="ts-item"><span class="ts-label">' + T("tx.summary.net") + "</span><span class='ts-value num ' + cls(net) + '\'>" + fmt(net) + "</span></div>";
    // Token 统计（仅后端 summary 提供时显示）：总 / 输入 / 缓存 / 输出
    if (s) {
      const t = (k) => fmtM(s[k] || 0);
      html +=
        '<div class="ts-item"><span class="ts-label">' + T("tx.summary.tokens") + "</span><span class='ts-value num '>" + t("tokens") + "</span></div>" +
        '<div class="ts-item"><span class="ts-label">' + T("tx.summary.input") + "</span><span class='ts-value num '>" + t("input_tokens") + "</span></div>" +
        '<div class="ts-item"><span class="ts-label">' + T("tx.summary.cached") + "</span><span class='ts-value num '>" + t("cached_tokens") + "</span></div>" +
        '<div class="ts-item"><span class="ts-label">' + T("tx.summary.output") + "</span><span class='ts-value num '>" + t("output_tokens") + "</span></div>";
    }
    $("#tx-summary").innerHTML = html;
  }

  function renderTransactions() {
    $$("#tx-tabs .tab").forEach((b) => b.classList.toggle("active", b.dataset.txTab === txTab));
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不 fallback D.TRANSACTIONS；
    // 加载失败 → 空态 + 重试；游客不可达（导航拦截）
    if (loggedIn() && !Live.transactions) {
      renderTxSummary([]);
      $("#tx-table").innerHTML = loadErrorHtml(T("tx.loadFail"), null, T("err.loadFail"));
      return;
    }
    let list = Live.transactions ? txsToView(Live.transactions.items || []) : [];
    // 交易汇总条：与 tab + 列筛选联动，与表格可见行一致（rant 20:39:30 B）
    renderTxSummary(filterRows(list, TX_COLUMNS, txTable.filters));
    buildDataTable({
      container: $("#tx-table"),
      columns: TX_COLUMNS,
      rows: list,
      state: txTable,
      onState: renderTransactions,
    });
  }

  // P2-B：按 tab 拉取交易（后端分页）
  async function loadTransactions() {
    if (!loggedIn()) return;
    const type = txTab === "all" ? "" : txTab;
    try {
      await liveLoad("transactions", "/api/transactions?type=" + type + "&page=1&page_size=100");
    } catch (e) { Live.transactions = null; /* 登录态降级空态 */ }
    renderTransactions();
  }

  // 交易记录导出 CSV（rant 20:46:57 E：Blob + a[download]，UTF-8 BOM，文件名 aitokenpool-transactions-YYYYMMDD.csv；导出当前筛选可见行）
  function exportTxCsv() {
    // 零 mock（rant 15:54:06）：登录态用后端数据，失败直接提示
    if (loggedIn() && !Live.transactions) { toast(T("tx.export.none"), "info"); return; }
    let list = Live.transactions ? txsToView(Live.transactions.items || []) : [];
    list = filterRows(list, TX_COLUMNS, txTable.filters); // 与表格可见行一致（含列筛选）
    if (!list.length) { toast(T("tx.export.none"), "info"); return; }
    const cell = (v) => { const s = String(v == null ? "" : v); return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s; };
    const headers = [T("tx.col.time"), T("tx.col.type"), T("tx.col.partner"), T("tx.col.tokens"), T("tx.col.pts"), T("tx.col.status")];
    const lines = list.map((t) => [t.time, txType(t.type), t.partner, t.tokens, t.pts, txStatus(t.status)].map(cell).join(","));
    const csv = "\uFEFF" + [headers.join(","), ...lines].join("\r\n"); // UTF-8 BOM，Excel 中文不乱码
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    const d = new Date();
    const p = (x) => String(x).padStart(2, "0");
    a.href = url;
    a.download = "aitokenpool-transactions-" + d.getFullYear() + p(d.getMonth() + 1) + p(d.getDate()) + ".csv";
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    toast(T("tx.export.ok", { n: list.length }), "success");
  }

  /* --- 数据表格键盘导航（rant 20:46:57 F：↑/↓ 行高亮 .row-active，Enter 触发主操作，Esc 清除） --- */
  const KBD_TABLE_IDS = ["mk-body", "share-body", "api-keys", "emp-body", "dept-body", "ops-body", "tx-table"];
  let kbd = { c: null, i: -1 }; // 当前激活的表格容器 + 高亮行下标

  function kbdRows(c) {
    if (!c) return [];
    const trs = c.tagName === "TBODY" ? c.querySelectorAll("tr") : c.querySelectorAll("tbody tr");
    return [].filter.call(trs, (tr) => tr && !(tr.classList && tr.classList.contains("mk-detail")));
  }
  function kbdClear() {
    if (kbd.c) kbdRows(kbd.c).forEach((tr) => tr.classList.remove("row-active"));
    kbd = { c: null, i: -1 };
  }
  function kbdSet(c, idx) {
    if (kbd.c && kbd.c !== c) kbdRows(kbd.c).forEach((tr) => tr.classList.remove("row-active"));
    const rows = kbdRows(c);
    if (!rows.length) { kbd = { c: null, i: -1 }; return; }
    idx = Math.max(0, Math.min(idx, rows.length - 1));
    if (kbd.c === c && kbd.i >= 0 && kbd.i < rows.length && kbd.i !== idx) rows[kbd.i].classList.remove("row-active");
    rows[idx].classList.add("row-active");
    kbd = { c: c, i: idx };
    if (rows[idx].scrollIntoView) rows[idx].scrollIntoView({ block: "nearest" });
  }
  function kbdMove(dir, c) {
    const cont = c || kbd.c;
    const rows = kbdRows(cont);
    if (!rows.length) return;
    let idx = kbd.c === cont ? kbd.i : -1;
    if (idx < 0) idx = dir > 0 ? -1 : rows.length; // 未激活时 ↓ 从首行、↑ 从末行开始
    kbdSet(cont, Math.max(0, Math.min(idx + dir, rows.length - 1)));
  }
  function kbdEnter() {
    const rows = kbdRows(kbd.c);
    if (!rows.length || kbd.i < 0) return;
    const tr = rows[kbd.i];
    if (!tr) return;
    // 主操作 = 行内第一个可用的操作按钮（排除行展开 +/-，含 .btn 但非 row-expand）
    const btn = tr.querySelector ? tr.querySelector("button.btn:not(.row-expand)") : null;
    if (btn && !btn.disabled) btn.click();
  }
  function kbdContainerFrom(t) {
    // 从事件目标向上找表格容器（tbody 本身或包 table 的 #tx-table），找不到沿用上次激活的表格
    if (t && t.closest) {
      const tb = t.closest("tbody");
      if (tb && KBD_TABLE_IDS.indexOf(tb.id) >= 0) return tb;
      const tbl = t.closest("table");
      if (tbl && tbl.parentNode && tbl.parentNode.id === "tx-table") return tbl.parentNode;
    }
    return kbd.c;
  }

  /* --- 通用 MRT 风格数据表格渲染器 ---
     cfg: { container, columns, rows, state, onState }
     columns: [{ key, title, sort?: "string"|"number", filter?: "text"|"select"|"number-range", options?, render? }]
     state:  { sort: [{key,dir}], filters: {key:val}, page, pageSize }（原地更新，跨页保留） */

  // 按列筛选条件过滤行（buildDataTable 与交易汇总条共用，保证汇总与表格可见行一致）
  function filterRows(rows, columns, filters) {
    return rows.filter((row) => {
      for (const key of Object.keys(filters)) {
        const fv = filters[key];
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
  }

  function buildDataTable(cfg) {
    const { container, columns, rows, state, onState } = cfg;

    // 1) 筛选
    let data = filterRows(rows, columns, state.filters);

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
      html += '<th' + (col.align === "num" ? ' class="num"' : "") + '><button type="button" class="th-sort" data-sort-key="' + esc(col.key) + '" title="' + T("tx.sort.title") + '">' +
        esc(typeof col.title === "function" ? col.title() : col.title) + arrow + "</button></th>";
    });
    html += "</tr><tr>";
    columns.forEach((col) => {
      const fv = state.filters[col.key] != null ? String(state.filters[col.key]) : "";
      if (col.filter === "select") {
        const opts = (typeof col.options === "function" ? col.options() : (col.options || [])).map((o) =>
          '<option value="' + esc(o) + '"' + (fv === String(o) ? " selected" : "") + ">" + esc(o) + "</option>").join("");
        html += '<td><select class="th-filter" data-filter-key="' + esc(col.key) + '"><option value="">' + T("common.all") + "</option>" + opts + "</select></td>";
      } else if (col.filter === "number-range") {
        const p = fv ? fv.split(":") : ["", ""];
        html += '<td class="range-filter"><input class="th-filter" data-filter-key="' + esc(col.key) + '" data-range="min" placeholder="' + T("tx.filter.min") + '" value="' + esc(p[0] || "") + '">' +
          '<input class="th-filter" data-filter-key="' + esc(col.key) + '" data-range="max" placeholder="' + T("tx.filter.max") + '" value="' + esc(p[1] || "") + '"></td>';
      } else if (col.filter) {
        html += '<td><input class="th-filter" data-filter-key="' + esc(col.key) + '" placeholder="' + T("tx.filter.placeholder") + '" value="' + esc(fv) + '"></td>';
      } else {
        html += "<td></td>";
      }
    });
    html += "</tr></thead><tbody>";
    if (!pageRows.length) html += '<tr><td colspan="' + columns.length + '" class="empty-cell">' + emptyState(T("tx.empty"), T("tx.empty.sub")) + "</td></tr>";
    pageRows.forEach((row) => {
      html += "<tr>";
      columns.forEach((col) => {
        html += "<td" + (col.align === "num" ? ' class="num"' : "") + ' data-label="' + esc(typeof col.title === "function" ? col.title() : col.title) + '">' +
          (col.render ? col.render(row) : esc(row[col.key] == null ? "" : row[col.key])) + "</td>";
      });
      html += "</tr>";
    });
    html += "</tbody></table>";

    // 5) 分页器 + 每页行数
    if (pages > 1) {
      html += '<div class="pager">';
      for (let i = 1; i <= pages; i++) html += '<button type="button" class="' + (i === state.page ? "active" : "") + '" data-p="' + i + '">' + i + "</button>";
      html += "<span>" + state.page + " / " + pages + " · " + T("tx.pager.count", { n: data.length }) + "</span></div>";
    }
    html += '<div class="pager-size">' + T("tx.pager.size") + ' <select data-page-size><option value="5">5</option><option value="10">10</option><option value="25">25</option><option value="50">50</option></select> ' + T("tx.pager.rows") + '</div>';

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

  function renderSettings() {
    // 账户昵称框：真实昵称（rant 2026-08-22T00:01:52：不再静态写「阿零」，避免覆盖真实昵称）
    const nick = $("#settings-nickname");
    if (nick) nick.value = D.USER.name || (D.USER.email ? D.USER.email.split("@")[0] : "");
    // 接入端点卡片：实时从配置/同源 fallback 读取（rant 2026-08-19T20:37:37）
    applyEndpointUrls();
    const rawQ = $("#ak-search").value || "";
    const q = rawQ.toLowerCase();
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不 fallback D.API_KEYS；
    // 加载失败 → 空态 + 重试；设置页仅登录可达
    if (loggedIn() && !Live.apiKeys) {
      $("#api-keys").innerHTML = loadErrorRow(6, T("settings.ak.loadFail"), T("err.loadFail"));
      pulseTbody($("#api-keys"));
      return;
    }
    // P2-B：登录 → 后端 /api/api-keys（key 已脱敏；完整 key 仅生成时可得）
    let list;
    if (Live.apiKeys) {
      list = Live.apiKeys.map((k) => ({
        id: k.id,
        fullKey: k.full_key || null,
        name: k.name || T("common.unnamed"),
        key: k.key,
        created: String(k.created_at || "").slice(0, 10),
        last: T("settings.ak.last.never"),
        status: k.status || "active",
      }));
    } else {
      list = [];
    }
    list = list.filter((k) => !q || k.name.toLowerCase().includes(q));
    $("#api-keys").innerHTML = list.length ? list.map((k, i) =>
      "<tr><td data-label='名字'><strong>" + hl(k.name, rawQ) + "</strong></td>" +
      "<td data-label='Key'><code>" + esc(Live.apiKeys ? k.key : "") + "</code></td>" +
      "<td data-label='创建时间'>" + esc(k.created) + "</td>" +
      "<td data-label='最近使用'>" + timeCell(k.last) + "</td>" +
      "<td data-label='状态'>" + (k.status === "active" ? '<span class="badge ok">' + T("settings.ak.status.active") + "</span>" : '<span class="badge dim">' + esc(k.status || "—") + "</span>") + "</td>" +
      "<td data-label='操作'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-copy='" + i + "'>" + T("settings.ak.copy") + "</button> " +
      "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-rename='" + i + "'>" + T("settings.ak.rename") + "</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-key-del='" + i + "'>" + T("settings.ak.del") + "</button></td></tr>"
    ).join("") : emptyRow(6, T("settings.ak.empty"), T("settings.ak.empty.sub"),
      '<button type="button" class="btn btn-ghost" data-new-key>' + T("settings.ak.empty.add") + "</button>");
    pulseTbody($("#api-keys"));
  }

  // P2-B：拉取 API Key 列表（登录时）
  async function loadApiKeys() {
    if (!loggedIn()) return;
    try {
      await liveLoad("apiKeys", "/api/api-keys");
    } catch (e) { Live.apiKeys = null; }
    renderSettings();
  }

  // 一键复制完整 key；file:// 下 clipboard API 受限 → 降级：临时 textarea 选中 + execCommand("copy")，仍失败则提示 Ctrl+C
  // 复制反馈（rant 15:50:05 B.10：复制后按钮短暂变「已复制」态）
  function copyKey(i) {
    if (!Live.apiKeys) return;
    const src = Live.apiKeys.map((k) => ({ ...k, fullKey: k.full_key || null }));
    const k = src[i];
    if (!k) return;
    // rant 2026-08-19T18:06:25：任意时候都可复制完整 key（列表 full_key 属主可见）；
    // 防御性兜底：full_key 缺失时退化为复制脱敏 key，绝不提示「仅生成时展示一次」
    const full = k.fullKey || k.key || "";
    const btn = document.querySelector('[data-key-copy="' + i + '"]');
    const flash = (ok) => {
      if (!btn) return;
      const orig = btn.innerHTML;
      btn.disabled = true;
      btn.innerHTML = ok ? T("common.copied") : T("common.ctrlC");
      setTimeout(() => { btn.disabled = false; btn.innerHTML = orig; }, 1200);
    };
    const okToast = () => { toast(T("settings.ak.copy.full", { name: k.name }), "success", { action: { label: T("settings.ak.copy.full.action"), onClick: gotoEndpointCard } }); flash(true); };
    const fallback = () => {
      const ta = document.createElement("textarea");
      ta.value = full;
      ta.style.cssText = "position:fixed;opacity:0";
      document.body.appendChild(ta);
      ta.select();
      let ok = false;
      try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
      document.body.removeChild(ta);
      if (ok) okToast();
      else { toast(T("common.copyHint")); flash(false); }
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(full).then(okToast).catch(fallback);
    } else {
      fallback();
    }
  }

  /* --- 接入端点（rant 2026-08-17T20:44:18：设置页展示 OpenAI/Anthropic 兼容 base URL） --- */
  // rant 2026-08-19T20:37:37：URL 不再硬编码域名——由配置 public_url 拼接（GET /api/config）；
  // 取不到配置 → fallback 同源 origin（同源部署天然正确）；渲染时实时读取，不依赖全局常量
  function endpointBase() {
    const u = Live.publicUrl ? String(Live.publicUrl).trim() : "";
    return u ? u.replace(/\/+$/, "") : location.origin;
  }
  function apiEndpoints() {
    const base = endpointBase();
    return [
      { tag: () => T("settings.ep.openai"), url: base + "/v1", desc: "Chat Completions · Cursor / Cline / Roo Code / OpenCode / OpenAI SDK" },
      { tag: () => T("settings.ep.anthropic"), url: base + "/anthropic", desc: "Messages API · Claude Code / Goose / OpenClaw" },
    ];
  }
  // 把动态端点写回设置页「接入方式」卡片（index.html 的 <code data-ep-url="i">）
  function applyEndpointUrls() {
    const eps = apiEndpoints();
    document.querySelectorAll("[data-ep-url]").forEach((el) => {
      const i = Number(el.getAttribute("data-ep-url"));
      const ep = eps[i];
      if (!ep) return;
      el.textContent = ep.url;
      el.setAttribute("data-endpoint", ep.url);
    });
  }

  // 复制端点 URL（复用 copyKey 的降级逻辑：clipboard API → execCommand → 提示 Ctrl+C）
  function copyEndpoint(i) {
    const ep = apiEndpoints()[i];
    if (!ep) return;
    const btn = document.querySelector('[data-ep-copy="' + i + '"]');
    const flash = (ok) => {
      if (!btn) return;
      const orig = btn.innerHTML;
      btn.disabled = true;
      btn.innerHTML = ok ? T("common.copied") : T("common.ctrlC");
      setTimeout(() => { btn.disabled = false; btn.innerHTML = orig; }, 1200);
    };
    const okToast = () => { toast(T("settings.ep.copied", { tag: ep.tag() }), "success"); flash(true); };
    const fallback = () => {
      const ta = document.createElement("textarea");
      ta.value = ep.url;
      ta.style.cssText = "position:fixed;opacity:0";
      document.body.appendChild(ta);
      ta.select();
      let ok = false;
      try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
      document.body.removeChild(ta);
      if (ok) okToast();
      else { toast(T("common.copyEpHint")); flash(false); }
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(ep.url).then(okToast).catch(fallback);
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
    const name = raw || T("common.unnamed");
    // P2-B：真实生成（POST /api/api-keys；设置页仅登录可达，零 mock rant 15:54:06）
    const btn = $("#ak-new-ok");
    withLoading(btn, () => {
      api.post("/api/api-keys", { name }).then(async () => {
        // 完整 key 由列表接口随行返回（full_key），无需会话缓存
        await liveLoad("apiKeys", "/api/api-keys");
        renderSettings();
        closeNewKeyInline();
        toast(T("settings.ak.gen.ok", { name: name }), "success");
      }).catch((err) => {
        toast((err && err.message) ? I18n.mapErr(err.message) : T("settings.ak.gen.fail"), "error");
      });
    });
  }

  // API Key 改名：行内编辑（替代原生输入弹窗）
  function renameKey(i) {
    const k = Live.apiKeys ? Live.apiKeys[i] : null;
    if (!k) return;
    const row = document.querySelector('#api-keys tr:nth-child(' + (i + 1) + ')');
    const cell = row ? row.children[0] : null;
    if (!cell) return;
    inlineForm(cell, {
      value: k.name,
      placeholder: T("settings.ak.rename.ph"),
      width: "160px",
      validate: (v) => v ? null : T("settings.ak.err.name"),
      onSubmit: (name) => { k.name = name; renderSettings(); toast(T("settings.ak.renamed", { name: name }), "success"); },
      onCancel: () => renderSettings(),
    });
  }

  function deleteKey(i) {
    if (!Live.apiKeys) return;
    const k = Live.apiKeys[i];
    if (!k) return;
    // P2-B：真实软删（DELETE /api/api-keys/:id）
    api.del("/api/api-keys/" + k.id).then(async () => {
      await loadApiKeys();
      toast(T("settings.ak.deleted", { name: k.name || T("common.unnamed") }), "success");
    }).catch((err) => {
      toast((err && err.message) ? I18n.mapErr(err.message) : T("settings.ak.del.fail"), "error");
    });
  }

  /* --- 管理员角色视图 --- */

  function renderAdmin() {
    const tab = $("#admin-tabs .tab.active").dataset.adminTab;
    $$(".admin-pane").forEach((p) => p.classList.toggle("hidden", p.dataset.adminPane !== tab));

    if (tab === "employees") {
      // 零 mock（rant 2026-08-19T15:54:06）：管理视图仅登录可达，绝不 fallback D.EMPLOYEES；
      // 加载失败 → 空态 + 重试
      if (!Live.adminUsers) {
        $("#emp-stats").innerHTML = "";
        $("#emp-body").innerHTML = loadErrorRow(7, T("admin.emp.loadFail"), T("err.loadFail"));
        pulseTbody($("#emp-body"));
        return;
      }
      // P2-B/P2-C：/api/admin/users（真实成员）+ /api/raise-requests（真实加额申请）
      const users = Live.adminUsers;
      const depts = Live.departments || [];
      const total = users.reduce((a, u) => a + (u.balance || 0), 0);
      $("#emp-stats").innerHTML = [
        stat(T("admin.emp.stats.members"), T("cnt.members", { n: users.length }), T("admin.emp.stats.members.sub.real")),
        stat(T("admin.emp.stats.total"), D.fmt(total) + " " + T("common.points"), T("admin.emp.stats.total.sub")),
        stat(T("admin.emp.stats.admins"), T("cnt.members", { n: users.filter((u) => u.role === "admin").length }), T("admin.emp.stats.admins.sub")),
        stat(T("admin.emp.stats.deps"), T("cnt.depts", { n: depts.length }), T("admin.emp.stats.deps.sub")),
      ].join("");
      $("#emp-body").innerHTML = users.map((u, i) =>
        "<tr data-emp-row='" + i + "'><td data-label='成员'><strong>" + esc(u.name || u.email) + "</strong>" +
        "<div class='muted' style='font-size:12px'>" + esc(u.email) + (u.role === "admin" ? T("admin.emp.role.admin") : u.role === "ops" ? T("admin.emp.role.ops") : "") + "</div></td>" +
        "<td data-label='角色'>" + (u.role === "admin" ? '<span class="badge ok">admin</span>' : u.role === "ops" ? '<span class="badge warn">ops</span>' : '<span class="badge dim">user</span>') + "</td>" +
        "<td data-label='部门'>" + (u.dept_name ? esc(u.dept_name) : '<span class="muted">' + T("common.unassigned") + "</span>") + "</td>" +
        '<td class="num" data-label="永久点数">' + D.fmt(u.balance || 0) + "</td>" +
        '<td class="num" data-label="赠送点数">' + D.fmt(u.gift_balance || 0) + "</td>" +
        '<td class="num" data-label="可用">' + D.fmt((u.balance || 0) + (u.gift_balance || 0)) + "</td>" +
        "<td data-label='操作'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-dept='" + i + "'>" + T("admin.emp.dept.change") + "</button> " +
        "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-topup='" + i + "'>" + T("admin.emp.topup") + "</button></td></tr>"
      ).join("");
      pulseTbody($("#emp-body"));
      renderRaiseRequests();
    } else if (tab === "usage") {
      // 零 mock（rant 15:54:06）：用量报表仅登录可达，绝不 fallback D.USAGE_MODEL/D.USAGE_EMP
      if (!Live.adminUsage) {
        $("#usage-model").innerHTML = loadErrorHtml(T("admin.usage.loadFail"), null, T("err.loadFail"));
        $("#usage-emp").innerHTML = "";
        $("#usage-dept").innerHTML = "";
        return;
      }
      // P2-C：/api/admin/usage（{users, models, departments} 三组聚合）
      const u = Live.adminUsage;
      const users = u.users || [], models = u.models || [], depts = u.departments || [];
      // 按模型（barRow 用 cost 归一）
      const maxMC = Math.max(1, ...models.map((m) => m.cost || 0));
      $("#usage-model").innerHTML = models.length
        ? models.map((m) => barRow(m.model, m.cost, maxMC, T("admin.usage.unit.yuan"))).join("")
        : '<div class="empty-state compact">' + EMPTY_ICON + "<p>" + T("admin.usage.empty.model") + "</p></div>";
      // 按成员（barRow 用 tokens 归一）
      const maxUT = Math.max(1, ...users.map((x) => x.month_tokens || 0));
      $("#usage-emp").innerHTML = users.length
        ? users.map((x) =>
            '<div class="mini-item"><div><div class="t">' + esc(x.name || x.email) + (x.dept_name ? '<span class="muted" style="font-size:11px"> · ' + esc(x.dept_name) + "</span>" : "") + "</div>" +
            '<div class="d">' + T("admin.usage.emp.row", { tokens: D.fmt(x.month_tokens || 0), cost: D.fmt(x.month_cost || 0) }) + "</div></div>" +
            '<div class="r"><span class="pts">' + T("cnt.calls", { n: x.month_calls || 0 }) + "</span><div class='d'>" + T("admin.usage.emp.calls") + "</div></div></div>"
          ).join("") + barRow(T("admin.usage.total"), users.reduce((a, x) => a + (x.month_tokens || 0), 0), maxUT, T("admin.usage.unit.tokens"))
        : '<div class="empty-state compact">' + EMPTY_ICON + "<p>" + T("admin.usage.empty.emp") + "</p></div>";
      // 按部门（barRow 用 cost 归一）
      const maxDC = Math.max(1, ...depts.map((d) => d.cost || 0));
      $("#usage-dept").innerHTML = depts.length
        ? depts.map((d) => barRow(d.name, d.cost, maxDC, T("admin.usage.unit.yuan"))).join("")
        : '<div class="empty-state compact">' + EMPTY_ICON + "<p>" + T("admin.usage.empty.dept") + "</p></div>";
    } else if (tab === "org") {
      renderOrg();
    } else if (tab === "models") {
      renderAdminModels();
    }
  }

  /* --- 模型管理（rant 2026-08-19T20:40:29：管理员模型信息 CRUD） --- */

  // 模型搜索过滤 + 表格渲染（数据来自 /api/admin/models；零 mock：加载失败 → 空态 + 重试）
  function renderAdminModels() {
    if (!Live.adminModels) {
      $("#model-body").innerHTML = loadErrorRow(7, T("admin.models.loadFail"), T("err.loadFail"));
      pulseTbody($("#model-body"));
      return;
    }
    const rawQ = $("#model-search").value || "";
    const q = rawQ.toLowerCase();
    const list = Live.adminModels.filter((m) => !q ||
      (m.provider || "").toLowerCase().includes(q) || (m.model || "").toLowerCase().includes(q));
    $("#model-body").innerHTML = list.length ? list.map((m, i) =>
      "<tr data-model-row='" + i + "'><td><strong>" + esc(m.provider) + "</strong></td>" +
      "<td><code>" + esc(m.model) + "</code></td>" +
      '<td class="num">' + D.fmt(m.input_per_m || 0) + "</td>" +
      '<td class="num">' + D.fmt(m.output_per_m || 0) + "</td>" +
      '<td class="num">' + fmtCtx(m.context_length || m.context_window || 0) + "</td>" +
      '<td class="num">' + fmtCtx(m.max_output || 0) + "</td>" +
      "<td>" + (m.vision ? '<span class="badge ok">' + T("admin.models.vision.yes") + "</span>" : '<span class="badge dim">' + T("admin.models.vision.no") + "</span>") + "</td>" +
      "<td data-label='操作'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-model-edit='" + i + "'>" + T("admin.models.edit") + "</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-model-del='" + i + "'>" + T("admin.models.del") + "</button></td></tr>"
    ).join("") : emptyRow(7, T("admin.models.empty"), T("admin.models.empty.sub"));
    pulseTbody($("#model-body"));
  }

  // 上下文数字格式化：1048576 → "1M"；0 → "—"
  function fmtCtx(n) {
    if (!n) return "—";
    if (n >= 1000000) return (n / 1000000).toFixed(n % 1000000 === 0 ? 0 : 1) + "M";
    if (n >= 1000) return (n / 1000).toFixed(n % 1000 === 0 ? 0 : 1) + "K";
    return String(n);
  }

  // 打开模型表单：i=null 新增；i=索引 编辑（预填）
  function openModelForm(i) {
    _editingModelId = (i === null) ? null : (Live.adminModels && Live.adminModels[i] ? Live.adminModels[i].id : null);
    const m = (i === null || !Live.adminModels) ? null : Live.adminModels[i];
    $("#model-form-title").innerHTML = m ? T("admin.models.form.title.edit") : T("admin.models.form.title.add");
    $("#model-form-provider").value = m ? m.provider : "";
    $("#model-form-model").value = m ? m.model : "";
    $("#model-form-currency").value = m ? m.currency : "USD";
    $("#model-form-in").value = m ? String(m.input_per_m || 0) : "0";
    $("#model-form-cachehit").value = m ? String(m.cache_hit_input_per_m || 0) : "0";
    $("#model-form-out").value = m ? String(m.output_per_m || 0) : "0";
    $("#model-form-peak-in").value = m ? String(m.peak_input_per_m || 0) : "0";
    $("#model-form-peak-cachehit").value = m ? String(m.peak_cache_hit_input_per_m || 0) : "0";
    $("#model-form-peak-out").value = m ? String(m.peak_output_per_m || 0) : "0";
    $("#model-form-ctx").value = m ? String(m.context_length || m.context_window || 0) : "0";
    $("#model-form-outmax").value = m ? String(m.max_output || 0) : "0";
    $("#model-form-vision").checked = m ? !!m.vision : false;
    clearFieldError($("#model-form-provider"));
    clearFieldError($("#model-form-model"));
    $("#model-form-card").hidden = false;
    $("#model-form-provider").focus();
  }

  // 提交模型表单（新增 POST / 编辑 PATCH）；校验后调真实 API
  function confirmModel() {
    const editingId = _editingModelId;
    const provider = String($("#model-form-provider").value).trim();
    const model = String($("#model-form-model").value).trim();
    const input = Number($("#model-form-in").value);
    const cachehit = Number($("#model-form-cachehit").value);
    const output = Number($("#model-form-out").value);
    const peakIn = Number($("#model-form-peak-in").value);
    const peakCache = Number($("#model-form-peak-cachehit").value);
    const peakOut = Number($("#model-form-peak-out").value);
    const ctx = Number($("#model-form-ctx").value);
    const outmax = Number($("#model-form-outmax").value);
    let firstErr = null;
    if (!provider) { setFieldError($("#model-form-provider"), T("admin.models.err.provider")); firstErr = firstErr || $("#model-form-provider"); }
    else clearFieldError($("#model-form-provider"));
    if (!model) { setFieldError($("#model-form-model"), T("admin.models.err.model")); firstErr = firstErr || $("#model-form-model"); }
    else clearFieldError($("#model-form-model"));
    if (input < 0 || cachehit < 0 || output < 0 || peakIn < 0 || peakCache < 0 || peakOut < 0) { toast(T("admin.models.err.price"), "error"); return; }
    if (firstErr) { firstErr.focus(); return; }
    const body = {
      provider, model,
      currency: $("#model-form-currency").value,
      input_per_m: input, output_per_m: output,
      cache_hit_input_per_m: cachehit,
      peak_input_per_m: peakIn, peak_output_per_m: peakOut,
      peak_cache_hit_input_per_m: peakCache,
      context_length: ctx || 0, max_output: outmax || 0,
      vision: $("#model-form-vision").checked ? 1 : 0,
    };
    const btn = $("#model-confirm");
    withLoading(btn, () => {
      const req = editingId ? api.patch("/api/admin/models/" + editingId, body) : api.post("/api/admin/models", body);
      req.then(async () => {
        await loadAdmin();
        $("#model-form-card").hidden = true;
        toast(editingId ? T("admin.models.saved") : T("admin.models.added"), "success");
      }).catch((err) => {
        toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.models.fail"), "error");
      });
    });
  }

  // 编辑中的模型 id 追踪（新增=null；编辑=行 id）
  let _editingModelId = null;

  // 打开编辑表单时记录 id（入口：表格「编辑」按钮）
  function editModelRow(i) {
    if (!Live.adminModels) return;
    openModelForm(i);
  }

  // 删除模型（行内二次确认 → DELETE）
  function deleteModel(i) {
    if (!Live.adminModels) return;
    const m = Live.adminModels[i];
    if (!m) return;
    api.del("/api/admin/models/" + m.id).then(async () => {
      await loadAdmin();
      toast(T("admin.models.deleted", { model: m.model }), "success");
    }).catch((err) => {
      toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.models.fail"), "error");
    });
  }

  // P2-B/P2-C：拉取管理员数据（users + usage + departments + raise-requests；登录且 role=admin 时）
  async function loadAdmin() {
    if (!loggedIn()) return;
    try { await liveLoad("adminUsers", "/api/admin/users"); } catch (e) { Live.adminUsers = null; }
    try { await liveLoad("adminUsage", "/api/admin/usage"); } catch (e) { Live.adminUsage = null; }
    try { await liveLoad("departments", "/api/admin/departments"); } catch (e) { Live.departments = null; }
    try { await liveLoad("raiseRequests", "/api/raise-requests"); } catch (e) { Live.raiseRequests = null; }
    try { await liveLoad("adminModels", "/api/admin/models"); } catch (e) { Live.adminModels = null; }
    renderAdmin();
  }

  /* --- 组织管理：部门列表 + 部门 CRUD + 每月点数分配 --- */

  // 成员改部门：行内下拉（选项来自后端部门 + "未分配"），确认后真实 PATCH（零 mock rant 15:54:06）
  function editEmpDept(i) {
    if (!Live.adminUsers || !Live.departments) { renderAdmin(); return; }
    const emp = Live.adminUsers[i];
    const row = document.querySelector('[data-emp-row="' + i + '"]');
    if (!emp || !row) return;
    const cell = row.children[2]; // 部门列（live 布局：成员/角色/部门/…）
    const depts = Live.departments;
    const sel = document.createElement("select");
    sel.className = "input";
    sel.style.cssText = "padding:4px 8px;font-size:12px;width:auto";
    const cur = emp.dept_id == null ? "" : emp.dept_id;
    sel.innerHTML = '<option value="">' + T("common.unassigned") + "</option>" +
      depts.map((d) => '<option value="' + d.id + '"' + (String(cur) === String(d.id) ? " selected" : "") + ">" + esc(d.name) + "</option>").join("");
    const ok = document.createElement("button");
    ok.className = "btn btn-primary";
    ok.style.cssText = "padding:4px 10px;font-size:12px";
    ok.textContent = T("common.confirm");
    const cancel = document.createElement("button");
    cancel.className = "btn btn-ghost";
    cancel.style.cssText = "padding:4px 10px;font-size:12px";
    cancel.textContent = T("common.cancel");
    const wrap = document.createElement("span");
    wrap.style.cssText = "display:inline-flex;gap:6px;align-items:center";
    wrap.append(sel, ok, cancel);
    cell.innerHTML = "";
    cell.appendChild(wrap);
    sel.focus();
    const done = () => {
      const v = sel.value;
      api.patch("/api/admin/users/" + emp.id, { dept_id: v === "" ? null : Number(v) }).then(async () => {
        await loadAdmin();
        const deptName = v === "" ? T("common.unassigned") : (depts.find((d) => String(d.id) === v) || {}).name;
        toast(T("admin.emp.dept.ok", { name: emp.name || emp.email, dept: deptName }), "success");
      }).catch((err) => {
        toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.emp.dept.fail"), "error");
        renderAdmin();
      });
    };
    ok.addEventListener("click", done);
    cancel.addEventListener("click", () => renderAdmin());
    sel.addEventListener("change", () => ok.focus());
  }

  function renderOrg() {
    const rawQ = $("#od-search").value || "";
    const q = rawQ.toLowerCase();
    // 零 mock（rant 15:54:06）：部门管理仅登录可达，绝不 fallback D.DEPARTMENTS；
    // 加载失败 → 空态 + 重试
    const src = Live.departments;
    if (!src) {
      $("#dept-stats").innerHTML = "";
      $("#dept-body").innerHTML = loadErrorRow(7, T("admin.org.loadFail"), T("err.loadFail"));
      pulseTbody($("#dept-body"));
      return;
    }
    const list = src.filter((d) => !q || d.name.toLowerCase().includes(q));

    const demoNote = $("#dept-demo-note");
    if (demoNote) demoNote.innerHTML = "";

    const totalQuota = src.reduce((a, d) => a + (d.quota || 0), 0);
    const totalUsed = src.reduce((a, d) => a + (d.month_cost || 0), 0);
    const unassigned = (Live.adminUsers || []).filter((u) => !u.dept_id).length;
    $("#dept-stats").innerHTML = [
      stat(T("admin.org.stats.depts"), T("cnt.depts", { n: src.length }), unassigned ? T("admin.org.stats.depts.sub", { n: unassigned }) : T("admin.org.stats.depts.sub2")),
      stat(T("admin.org.stats.monthly"), D.fmt(totalQuota) + " " + T("common.points"), T("admin.org.stats.monthly.sub")),
      stat(T("admin.org.stats.used"), D.fmt(totalUsed) + " " + T("common.points"), totalQuota ? T("admin.org.stats.used.sub", { p: Math.round((totalUsed / totalQuota) * 100) }) : "—"),
      stat(T("admin.org.stats.remain"), D.fmt(totalQuota - totalUsed) + " " + T("common.points"), T("admin.org.stats.remain.sub")),
    ].join("");

    $("#dept-body").innerHTML = list.length ? list.map((d, i) => {
      const used = d.month_cost || 0;
      const members = d.member_count || 0;
      const pct = d.quota > 0 ? used / d.quota : 0;
      const st = pct >= 1 ? '<span class="badge danger">' + T("common.exhausted") + "</span>" : pct > 0.9 ? '<span class="badge warn">' + T("common.nearLimit") + "</span>" : '<span class="badge ok">' + T("common.normal") + "</span>";
      return "<tr><td data-label='部门'><strong>" + hl(d.name, rawQ) + "</strong></td>" +
        '<td class="num" data-label="成员数">' + T("cnt.members", { n: members }) + "</td>" +
        '<td class="num" data-label="月分配（点数）">' + D.fmt(d.quota) + " " + T("common.points") + "</td>" +
        '<td class="num" data-label="已用">' + D.fmt(used) + " " + T("common.points") + "</td>" +
        '<td class="num" data-label="剩余">' + D.fmt(d.quota - used) + " " + T("common.points") + "</td>" +
        "<td data-label='状态'>" + st + "</td>" +
        "<td data-label='操作'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-dept-edit='" + i + "'>" + T("common.edit") + "</button> " +
        "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-dept-del='" + i + "'>" + T("common.delete") + "</button></td></tr>";
    }).join("") : emptyRow(7, T("admin.org.empty"), T("admin.org.empty.sub"),
      '<button type="button" class="btn btn-ghost" data-dept-clear-search>' + T("admin.org.clearSearch") + "</button>");
    pulseTbody($("#dept-body"));
  }

  /* --- 部门添加/编辑：行内展开表单（UI 原则：少用弹窗，优先行内交互；继承 rant 10:59:47 的可靠响应） --- */

  let deptEditIndex = null; // null = 添加，数字 = 编辑的部门索引

  function openDeptForm(i) {
    deptEditIndex = (i == null ? null : i);
    const src = Live.departments || [];
    const d = (i == null ? null : src[i]);
    $("#dept-form-title").innerHTML = d
      ? T("admin.org.edit.title")
      : T("admin.org.add.title");
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
    if (!name) { setFieldError($("#dept-form-name"), T("admin.org.err.name")); firstErr = firstErr || $("#dept-form-name"); }
    else clearFieldError($("#dept-form-name"));
    if (!rawQ || !Number.isInteger(quota) || quota <= 0) { setFieldError($("#dept-form-quota"), T("admin.org.err.quota")); firstErr = firstErr || $("#dept-form-quota"); }
    else clearFieldError($("#dept-form-quota"));
    if (firstErr) { firstErr.focus(); return; }
    // 零 mock（rant 15:54:06）：部门 CRUD 全走真实 API（admin 仅登录可达）
    const src = Live.departments || [];
    if (deptEditIndex == null) {
      if (src.some((d) => d.name === name)) { setFieldError($("#dept-form-name"), T("admin.org.err.dup", { name: name })); $("#dept-form-name").focus(); return; }
      api.post("/api/admin/departments", { name, quota }).then(async () => {
        await loadAdmin();
        $("#dept-form-card").hidden = true;
        toast(T("admin.org.add.ok", { name: name, quota: D.fmt(quota) }), "success");
      }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.org.add.fail"), "error"));
    } else {
      const d = src[deptEditIndex];
      if (!d) return;
      if (name !== d.name && src.some((x) => x.name === name)) { setFieldError($("#dept-form-name"), T("admin.org.err.dup", { name: name })); $("#dept-form-name").focus(); return; }
      api.patch("/api/admin/departments/" + d.id, { name, quota }).then(async () => {
        await loadAdmin();
        $("#dept-form-card").hidden = true;
        toast(T("admin.org.edit.ok", { name: name, quota: D.fmt(quota) }), "success");
      }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.org.edit.fail"), "error"));
    }
  }

  function deleteDept(i) {
    const src = Live.departments || [];
    const d = src[i];
    if (!d) return;
    api.del("/api/admin/departments/" + d.id).then(async () => {
      await loadAdmin();
      toast(T("admin.org.del.ok", { name: d.name }), "success");
    }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.org.del.fail"), "error"));
  }

  function barRow(name, pts, max, unit) {
    const pct = Math.round((pts / max) * 100);
    return '<div class="bar-row"><div class="bar-label"><span>' + esc(name) + '</span><span class="n">' + D.fmt(pts) + " " + unit + "</span></div>" +
      '<div class="bar"><i style="width:' + pct + '%"></i></div></div>';
  }

  /* --- 平台运营者视图（US-运营1 / US-运营2：运营者 = 宿主本人，职责仅两项） --- */

  function renderOps() {
    const tab = $("#ops-tabs .tab.active").dataset.opsTab;
    $$(".ops-pane").forEach((p) => p.classList.toggle("hidden", p.dataset.opsPane !== tab));

    if (tab === "runtime") {
      // 零 mock（rant 15:54:06）：运营视图仅登录+role=ops 可达，绝不 fallback D.TRANSACTIONS/D.SHARINGS；
      // 加载失败 → 空态 + 重试
      const note = $("#ops-demo-note");
      if (note) note.innerHTML = "";
      if (!Live.opsRuntime) {
        $("#ops-stats").innerHTML = loadErrorHtml(T("ops.loadFail"), null, T("err.loadFail"));
        return;
      }
      // P2-C：/api/ops/runtime 真实聚合
      const rt = Live.opsRuntime;
      $("#ops-stats").innerHTML = [
        stat(T("ops.stats.status"), '<span class="badge ok">' + T("common.online") + "</span>", T("ops.stats.status.sub")),
        stat(T("ops.stats.users"), T("cnt.people", { n: rt.users }), T("ops.stats.users.sub")),
        stat(T("ops.stats.keys"), T("cnt.keys", { n: rt.active_keys }), T("ops.stats.keys.sub.on")),
        stat(T("ops.stats.calls"), T("cnt.calls", { n: rt.month_calls }), T("ops.stats.calls.sub")),
        stat(T("ops.stats.in"), "+" + D.fmt(rt.month_in) + " " + T("common.points"), T("ops.stats.in.sub")),
        stat(T("ops.stats.out"), "-" + D.fmt(rt.month_out) + " " + T("common.points"), T("ops.stats.out.sub")),
      ].join("");
      return;
    }

    // tab === "users"：成员充值（零 mock，rant 15:54:06）
    if (!Live.opsUsers) {
      $("#ops-body").innerHTML = loadErrorRow(4, T("ops.loadFail"), T("err.loadFail"));
      pulseTbody($("#ops-body"));
      return;
    }
    const src = Live.opsUsers;
    const rawQ = $("#ops-search").value || "";
    const q = rawQ.toLowerCase();
    const list = src.filter((u) => !q || u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q));
    $("#ops-body").innerHTML = list.length ? list.map((u) =>
      "<tr><td data-label='用户'><strong>" + hl(u.name, rawQ) + "</strong></td>" +
      "<td data-label='邮箱'>" + hl(u.email, rawQ) + "</td>" +
      '<td class="num" data-label="余额（点数）">' + D.fmt(u.balance || 0) + " " + T("common.points") + "</td>" +
      "<td data-label='操作'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-ops-topup='" + u.id + "'>" + T("ops.users.topup") + "</button></td></tr>"
    ).join("") : emptyRow(4, T("ops.users.empty"), T("ops.users.empty.sub"));
    pulseTbody($("#ops-body"));
  }

  // P2-C：拉取运营者数据（runtime + users；登录且 role=ops 时）
  async function loadOps() {
    if (!loggedIn()) return;
    try { await liveLoad("opsRuntime", "/api/ops/runtime"); } catch (e) { Live.opsRuntime = null; }
    try { await liveLoad("opsUsers", "/api/ops/users"); } catch (e) { Live.opsUsers = null; }
    renderOps();
  }

  // 运营者给用户充值：行内编辑（替代原生输入弹窗，Enter 确认 / Esc 取消）
  function inlineOpsTopup(u, btn) {
    const row = btn.closest("tr");
    if (!row) return;
    const cell = row.children[3];
    inlineForm(cell, {
      value: "100",
      placeholder: T("ops.users.topup.ph"),
      type: "number",
      width: "120px",
      validate: (raw) => {
        const amt = Math.round(Number(raw) * 100) / 100;
        return (!raw || isNaN(amt) || amt <= 0) ? T("ops.users.err.amount") : null;
      },
      onSubmit: (raw) => {
        const amt = Math.round(Number(raw) * 100) / 100;
        if (!Live.opsUsers) { toast(T("ops.users.topup.fail"), "error"); return; }
        // P2-C：真实充值（POST /api/ops/credits；零 mock rant 15:54:06）
        api.post("/api/ops/credits", { user_id: u.id, amount: amt }).then(async () => {
          await loadOps();
          renderOps();
          if (u.email === D.USER.email) {
            try { const w = await api.get("/api/wallet"); if (w) D.USER.balance = w.balance; $("#side-balance").textContent = D.fmt(D.USER.balance); bump($("#side-balance")); } catch (e) {}
          }
          toast(T("ops.users.topup.ok", { name: u.name, amt: D.fmt(amt) }), "success");
        }).catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("ops.users.topup.fail"), "error"));
      },
      onCancel: () => renderOps(),
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
  // 时区（rant 2026-08-19T20:45:32）：后端返回 UTC ISO 带 Z（'YYYY-MM-DDTHH:MM:SSZ'），
  // 按 UTC 解析；旧格式 'YYYY-MM-DD HH:MM:SS' 同样视为 UTC（补 Z）；
  // 游客 mock 的 'MM-DD HH:mm'（本地时间）保持按本地解析；非标准格式原样返回
  function timeAgo(s) {
    if (!s) return "";
    const p2 = (x) => String(x).padStart(2, "0");
    const full = String(s);
    // UTC ISO（'YYYY-MM-DDTHH:MM:SSZ'，可含秒/毫秒）
    let m = full.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::\d{2}(?:\.\d+)?)?Z$/);
    let utc = false;
    if (m) {
      utc = true;
    } else {
      // 旧后端格式 'YYYY-MM-DD[ HH:MM[:SS]]' → 视为 UTC
      m = full.match(/^(\d{4})-(\d{2})-(\d{2})(?:\s+(\d{2}):(\d{2})(?::\d{2})?)?$/);
      if (m) utc = true;
    }
    if (m) {
      const Y = +m[1], MM = +m[2], DD = +m[3], HH = +(m[4] || 0), mm = +(m[5] || 0);
      const d = utc ? new Date(Date.UTC(Y, MM - 1, DD, HH, mm)) : null;
      if (d && !isNaN(d.getTime())) {
        const min = Math.floor((Date.now() - d.getTime()) / 60000);
        if (min < 1) return T("time.justNow");
        if (min < 60) return T("time.minAgo", { n: min });
        if (min < 60 * 24) return T("time.hourAgo", { n: Math.floor(min / 60) });
        const nowU = new Date();
        const utcToday = Date.UTC(nowU.getUTCFullYear(), nowU.getUTCMonth(), nowU.getUTCDate());
        const dayDiff = Math.floor((utcToday - Date.UTC(Y, MM - 1, DD)) / 86400000);
        if (dayDiff === 1) return T("time.yesterday");
        return p2(MM) + "-" + p2(DD);
      }
    }
    // 游客 mock：'MM-DD HH:mm' 按本地时间解析（默认今年）
    m = full.match(/^(\d{2})-(\d{2})\s+(\d{2}):(\d{2})$/);
    if (m) {
      const now = new Date();
      const d = new Date(now.getFullYear(), +m[1] - 1, +m[2], +m[3], +m[4]);
      if (isNaN(d.getTime())) return full;
      const min = Math.floor((now - d) / 60000);
      if (min < 1) return T("time.justNow");
      if (min < 60) return T("time.minAgo", { n: min });
      if (min < 60 * 24) return T("time.hourAgo", { n: Math.floor(min / 60) });
      const dayDiff = Math.floor(
        (new Date(now.getFullYear(), now.getMonth(), now.getDate()) - new Date(now.getFullYear(), +m[1] - 1, +m[2])) / 86400000);
      if (dayDiff === 1) return T("time.yesterday");
      return p2(+m[1]) + "-" + p2(+m[2]);
    }
    return full;
  }

  // 时间单元格：相对时间展示 + title 悬停显示本地化绝对时间（rant 2026-08-19T20:45:32）
  function timeCell(s) {
    if (!s) return "";
    const t = String(s);
    const iso = /^(\d{4})-(\d{2})-(\d{2})[T ]/.test(t);
    let title = t;
    if (iso) {
      const d = new Date(iso && t.includes("T") ? t : t.replace(" ", "T") + "Z");
      if (!isNaN(d.getTime())) title = d.toLocaleString();
    }
    return '<span class="timeago" title="' + esc(title) + '">' + esc(timeAgo(s)) + "</span>";
  }

  function openChat(id) {
    // 零 mock（rant 15:54:06）：登录态绝不回退 D.MARKET
    if (loggedIn() && !Live.models) { toast(T("err.loadFail"), "error"); return; }
    const m = (Live.models ? modelsToView(Live.models) : D.MARKET).find((x) => x.id === id);
    if (!m) return;
    if (!m.avail) { toast(T("chat.busy"), "error"); return; }
    markRecentUsed(id); // 记录最近使用（rant 20:46:57 D：去重 + 置顶，最多 5 个）
    renderRecent();     // 立即刷新最近使用 chips
    chatModel = m;
    $("#chat-title").textContent = T("chat.title", { model: m.model });
    $("#chat-meta").textContent = T("chat.meta", { in: D.fmt(m.in), out: D.fmt(m.out), balance: D.fmt(D.USER.balance) }) +
      (m.multi ? T("chat.meta.multi") : "");
    $("#chat-log").innerHTML = '<p class="muted chat-tip">' + T("chat.tip") + "</p>";
    $("#chat-input").value = "";
    $("#chat-modal").classList.remove("hidden");
    $("#chat-input").focus();
  }

  function closeChat() {
    $("#chat-modal").classList.add("hidden");
    chatModel = null;
  }

  // P2-B：真实消费链路 —— POST /v1/chat/completions（最小占位请求，stream=false）
  async function consumeModel(id) {
    // 零 mock（rant 15:54:06）：登录态绝不回退 D.MARKET
    if (loggedIn() && !Live.models) { toast(T("err.loadFail"), "error"); return; }
    const src = Live.models ? modelsToView(Live.models) : [];
    const m = src.find((x) => x.id === id);
    if (!m) return;
    if (!m.avail) { toast(T("chat.busy"), "error"); return; }
    if (!loggedIn()) { toast(T("chat.login.need"), "error"); return; }
    markRecentUsed(id); // 记录最近使用（rant 20:46:57 D）
    renderRecent();
    const btn = document.querySelector('[data-use-model="' + id + '"]');
    if (btn) { btn.disabled = true; btn.textContent = T("chat.calling"); }
    try {
      await api.post("/v1/chat/completions", {
        model: m.model,
        messages: [{ role: "user", content: "ping" }],
        stream: false,
      });
      toast(T("chat.consume.live", { model: m.model }), "success");
      await refreshBalanceAndView(); // 刷新钱包 + 当前视图
    } catch (err) {
      // 余额不足（402）/ 暂无可用 key（503）等后端错误直接展示
      toast((err && err.message) ? I18n.mapErr(err.message) : T("chat.consume.fail"), "error");
      await refreshWallet();
      $("#side-balance").textContent = D.fmt(D.USER.balance);
    } finally {
      if (btn) { btn.disabled = false; btn.textContent = T("mk.use"); }
    }
  }

  function sendChat() {
    const m = chatModel;
    if (!m) return;
    const text = $("#chat-input").value.trim();
    if (!text) { toast(T("chat.err.empty"), "error"); return; }
    // 模拟一次调用：0.19M tokens，按输出参考价计费（v1.6：消费点数可为小数，保留 2 位）
    const tokens = 0.19;
    const cost = Math.round(tokens * m.out * 100) / 100;
    if (D.USER.balance < cost) {
      toast(T("chat.err.balance", { cost: D.fmt(cost), balance: D.fmt(D.USER.balance) }), "error");
      return;
    }
    D.USER.balance = Math.round((D.USER.balance - cost) * 100) / 100;
    // 聊天为模拟交互（P2-D 候选：chat-modal 流式网关）；D.TRANSACTIONS 已移除（rant 15:54:06），不写明细
    const log = $("#chat-log");
    if (log.querySelector(".chat-tip")) log.innerHTML = "";
    log.innerHTML +=
      '<div class="chat-msg user"><div class="bubble">' + esc(text) + "</div></div>" +
      '<div class="chat-msg bot"><div class="bubble">' + T("chat.reply") + "</div></div>";
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    bump($("#side-balance")); // 消费扣款 → 余额跳动（rant 18:06:09 E）
    $("#chat-meta").textContent = T("chat.meta.after", { cost: D.fmt(cost), balance: D.fmt(D.USER.balance) });
    $("#chat-input").value = "";
    toast(T("chat.consume.ok", { cost: D.fmt(cost) }), "success");
  }

  /* ---------------- 游客模式（US-1：未登录浏览市场） ---------------- */

  function enterGuest() {
    isGuest = true;
    pendingHashView = null;
    activeView = "marketplace";
    $("#login-view").classList.add("hidden");
    $("#app").classList.remove("hidden");
    document.querySelector(".user-chip").classList.add("hidden");
    renderNav();
    switchView("marketplace");
    toast(T("guest.enter"), "info");
  }

  function exitGuest() {
    isGuest = false;
    $("#app").classList.add("hidden");
    document.querySelector(".user-chip").classList.remove("hidden");
    $("#login-view").classList.remove("hidden");
  }

  /* ---------------- 会话（P2-A：/api/me + /api/wallet） ---------------- */

  // 拉当前用户信息 + 钱包余额（替代 mock D.USER.*）；余额失败 → 0 + 红色提示
  async function loadSession() {
    const me = await api.get("/api/me");
    D.USER.name = (me && me.name) || (me && me.email ? me.email.split("@")[0] : T("common.user"));
    D.USER.email = (me && me.email) || D.USER.email;
    D.USER.role = (me && me.role) || "user";
    // 服务端配置（rant 2026-08-19T20:37:37）：public_url → 接入端点 base；失败 → 同源 fallback
    try {
      const cfg = await api.get("/api/config");
      if (cfg && cfg.public_url) Live.publicUrl = String(cfg.public_url).trim();
    } catch (e) { Live.publicUrl = null; }
    try {
      const w = await api.get("/api/wallet");
      D.USER.balance = (w && typeof w.available === "number") ? w.available : (w ? w.balance : 0);
    } catch (e) {
      D.USER.balance = 0;
      toast(T("login.balance.fail"), "error");
    }
  }

  // 左下角用户芯片：真实昵称 + 头像首字符（rant 2026-08-22T00:01:52：去掉「阿零」硬编码）
  function renderUserChip() {
    const name = D.USER.name || (D.USER.email ? D.USER.email.split("@")[0] : "");
    $("#side-name").textContent = name;
    $("#side-avatar").textContent = name ? Array.from(name)[0] : "?";
  }

  // 进入主界面（登录成功 / 会话恢复共用）
  function enterApp() {
    isGuest = false;
    pendingHashView = null;
    document.querySelector(".user-chip").classList.remove("hidden");
    $("#login-view").classList.add("hidden");
    $("#app").classList.remove("hidden");
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    renderUserChip();
    renderNav();
    // URL hash 路由：登录后恢复刷新前的视图（无 hash 则仪表盘）
    switchView(viewFromHash() || "dashboard");
    maybeStartTour(); // 首次登录引导（rant 20:46:57 A：atp-tour-done 未标记才触发）
  }

  // api.js 401 钩子：token 失效 → 清 token 回登录页
  window.__atpLogout = () => {
    api.clearToken();
    exitGuest();
    toast(T("login.session.expired"), "error");
  };

  /* ---------------- P2-B 真实 API 数据层（各视图 mock 数据逐步替换为后端） ---------------- */

  // 各视图真实数据缓存：登录且加载成功后使用；游客 / 失败降级 mock
  const Live = {
    publicUrl: null,     // GET /api/config → public_url（接入端点 base，rant 2026-08-19T20:37:37）
    models: null,        // GET /api/models 原始数组
    plans: null,         // GET /api/plans 原始数组（上架表单数据源；rant 16:14:21 Bug 1）
    sharings: null,      // GET /api/sharings 原始数组
    transactions: null,  // GET /api/transactions → {items,total,...}
    wallet: null,        // GET /api/wallet
    dashboard: null,     // GET /api/dashboard
    apiKeys: null,       // GET /api/api-keys
    adminUsers: null,    // GET /api/admin/users
    adminUsage: null,    // GET /api/admin/usage → {users, models, departments}
    departments: null,   // P2-C GET /api/admin/departments
    raiseRequests: null, // P2-C GET /api/raise-requests（admin 视角全部）
    opsRuntime: null,    // P2-C GET /api/ops/runtime
    opsUsers: null,      // P2-C GET /api/ops/users
    adminModels: null,   // rant 20:40:29 GET /api/admin/models（管理表格数据源）
  };

  function loggedIn() { return !!api.getToken() && !isGuest; }

  // 拉取并缓存；失败抛错（调用方决定降级）
  async function liveLoad(key, path) {
    const data = await api.get(path);
    Live[key] = data;
    return data;
  }

  // 通用降级渲染：加载失败 → 空态 + 重试按钮（不白屏）
  function loadErrorHtml(emptyLabel, retryFn, retryLabel) {
    return '<div class="empty-state">' + EMPTY_ICON +
      "<p>" + esc(emptyLabel) + "</p>" +
      '<p class="muted">' + esc(retryLabel || T("err.loadFail")) + '</p>' +
      '<button type="button" class="btn btn-ghost" data-live-retry>' + T("common.retry") + "</button></div>";
  }
  // 重试按钮委托（容器级）
  function bindLiveRetry(containerId, fn) {
    const c = document.getElementById(containerId);
    if (!c) return;
    c.addEventListener("click", (e) => {
      if (e.target.closest("[data-live-retry]")) fn();
    });
  }

  // tbody 容器专用降级行：<tr><td> 内嵌 loadErrorHtml（div 直接进 tbody 会被浏览器提升到表外，破坏布局与重试委托）
  function loadErrorRow(colspan, emptyLabel, retryLabel) {
    return '<tr><td class="empty-cell" colspan="' + colspan + '">' + loadErrorHtml(emptyLabel, null, retryLabel) + "</td></tr>";
  }

  // 后端 models → 视图行（点数按 points_per_unit=1、锚定 CNY 折算；USD 价 ×7.2；ctx 来自 models.context_window；
  // multi=available_keys>=2 真实计算；success 后端暂无字段 → null，视图不渲染假成功率；
  // peak 高峰时段价（rant 2026-08-20T11:58:40）：peak_input_per_m>0 → 启用高峰计费，展示 ×N 标注）
  // 零 mock（rant 2026-08-19T15:54:06）：不读 data.js MARKET 兜底
  function modelsToView(list) {
    return list.map((m, i) => {
      const cny = m.currency === "CNY";
      const mult = cny ? 1 : 7.2;
      const peak = (m.peak_input_per_m || 0) > 0;
      return {
        id: i,
        provider: m.provider,
        model: m.model,
        in: Math.round(m.input_per_m * mult * 1e5) / 1e5,
        out: Math.round(m.output_per_m * mult * 1e5) / 1e5,
        peak: peak,
        peakIn: peak ? Math.round(m.peak_input_per_m * mult * 1e5) / 1e5 : 0,
        peakOut: peak ? Math.round(m.peak_output_per_m * mult * 1e5) / 1e5 : 0,
        peakMult: peak && (m.input_per_m || 0) > 0 ? Math.round((m.peak_input_per_m / m.input_per_m) * 10) / 10 : 0,
        ctx: m.context_window || 0,
        avail: m.available_keys > 0,
        multi: (m.available_keys || 0) >= 2,
        success: null,
        live: true,
      };
    });
  }

  // 后端 sharings → 视图行（字段对齐 mock：earned=earn、price 用 autoPrice、time 占位）
  function sharingsToView(list) {
    return list.map((s) => {
      let days = [];
      try { days = JSON.parse(s.available_days || "[]"); } catch (e) { days = []; }
      return {
        id: s.id,
        provider: s.provider,
        plan: s.plan || "API",
        model: s.model,
        key: s.key,
        quota: s.quota,
        used: s.used,
        price: autoPrice(s.model),
        earned: s.earn || 0,
        status: s.status,
        time: "",
        note: s.note || "",
        available: days.length ? { days, start: s.available_start || "", end: s.available_end || "" } : null,
      };
    });
  }

  // 后端 transactions items → 视图行（partner=counterpart、tokens 格式化、detail 用模型）
  // rant 2026-08-21T14:53:20：单次调用 token 明细（输入/缓存命中/输出）随行展示
  const fmtTokens = (n) => (typeof n === "number" && n > 0 ? (n >= 1e6 ? (n / 1e6).toFixed(2) + "M" : String(Math.round(n))) : "0");
  function txsToView(items) {
    return items.map((t) => {
      let tokens = "—";
      if (typeof t.tokens === "number" && t.tokens > 0) {
        tokens = t.tokens >= 1e6 ? (t.tokens / 1e6).toFixed(2) + "M" : String(Math.round(t.tokens));
      }
      // 明细仅当后端提供了拆分且确有输出/缓存（旧记录 0/0 不显示，保持整洁）
      const input = typeof t.input_tokens === "number" ? t.input_tokens : null;
      const cached = typeof t.cached_tokens === "number" ? t.cached_tokens : null;
      const output = typeof t.output_tokens === "number" ? t.output_tokens : null;
      const hasBrk = input !== null && cached !== null && output !== null && (cached > 0 || output > 0);
      const tokenDetail = hasBrk
        ? '<span class="brk-i">' + esc(T("tx.brk.input")) + " " + fmtTokens(input) + "</span>" +
          '<span class="brk-c">' + esc(T("tx.brk.cache")) + " " + fmtTokens(cached) + "</span>" +
          '<span class="brk-o">' + esc(T("tx.brk.output")) + " " + fmtTokens(output) + "</span>"
        : "";
      return {
        id: t.id,
        time: (t.time || "").replace("T", " ").slice(0, 16),
        type: t.type,
        partner: t.counterpart || "—",
        detail: t.model ? "消费 · " + t.model : "交易",
        tokens,
        tokenDetail,
        pts: t.pts,
        status: t.status || "成功",
      };
    });
  }

  // 刷新钱包缓存（登录后）；返回最新 available
  async function refreshWallet() {
    try {
      Live.wallet = await api.get("/api/wallet");
      if (typeof Live.wallet.available === "number") D.USER.balance = Live.wallet.available;
      return Live.wallet.available;
    } catch (e) { return D.USER.balance; }
  }

  // 刷新侧边栏余额 + 当前视图（交易/仪表盘等消费后联动）
  async function refreshBalanceAndView() {
    await refreshWallet();
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    if (activeView) switchView(activeView, { sync: false });
  }

  /* ---------------- 事件 ---------------- */

  function bindEvents() {
    // 游客浏览（US-1：登录页入口 → 免登录进入市场）
    $("#guest-browse-btn").addEventListener("click", enterGuest);

    // 登录（P2-A：对接 POST /api/auth/login；失败行内报错；成功存 token + 拉会话）
    $("#login-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const email = $("#login-email").value.trim();
      const pass = $("#login-pass").value;
      let firstErr = null;
      if (!email) { setFieldError($("#login-email"), T("login.err.email")); firstErr = firstErr || $("#login-email"); }
      else clearFieldError($("#login-email"));
      if (!pass) { setFieldError($("#login-pass"), T("login.err.pass")); firstErr = firstErr || $("#login-pass"); }
      else clearFieldError($("#login-pass"));
      if (firstErr) { firstErr.focus(); return; }
      // 记住我（P2-A：token 存 localStorage 长期 / sessionStorage 关闭失效）
      try { localStorage.setItem("atp-remember", $("#login-remember").checked ? "1" : "0"); } catch (err) { /* 隐私模式忽略 */ }
      const btn = e.target.querySelector('button[type="submit"]');
      if (btn) { btn.disabled = true; btn.textContent = T("login.logging"); }
      try {
        const r = await api.post("/api/auth/login", { email, password: pass });
        api.saveToken(r.api_key);
        await loadSession(); // /api/me → 用户信息；/api/wallet → 余额
        enterApp();
        toast(T("login.welcome", { name: D.USER.name || email }), "success");
      } catch (err) {
        if (err && err.status === 401) {
          setFieldError($("#login-pass"), T("login.err.bad"));
          clearFieldError($("#login-email"));
          $("#login-pass").focus();
        } else if (err && err.status === 403) {
          // 邮箱未验证 → 切验证码界面 + 预填邮箱 + 自动发码（rant 2026-08-21T12:31:48）
          $("#verify-email").value = email;
          showAuthForm("verify");
          const rb = $("#resend-btn");
          if (rb) rb.click(); // 立即发验证码（60s 限频由后端 429 兜底提示）
        } else {
          toast((err && err.message) ? I18n.mapErr(err.message) : T("login.err.fail"), "error");
        }
      } finally {
        if (btn) { btn.disabled = false; btn.textContent = T("login.submit"); }
      }
    });

    $("#logout-btn").addEventListener("click", () => {
      api.clearToken();
      exitGuest();
      toast(T("logout.done"), "info");
    });

    /* ---- 注册 / 邮箱验证 / 找回密码（rant 2026-08-19T14:36:19；2026-08-20 找回密码）----
       ⚠️ 事件委托（rant 2026-08-20：i18n applyStatic 用 innerHTML 重建 login-foot
       节点，直接绑定会丢失 listener；委托到 document 免疫） */
    const loginFormEl = $("#login-form");
    const registerFormEl = $("#register-form");
    const verifyFormEl = $("#verify-form");
    const forgotFormEl = $("#forgot-form");

    function showAuthForm(which) {
      // which: "login" | "register" | "verify" | "forgot"
      loginFormEl.classList.toggle("hidden", which !== "login");
      registerFormEl.classList.toggle("hidden", which !== "register");
      verifyFormEl.classList.toggle("hidden", which !== "verify");
      if (forgotFormEl) forgotFormEl.classList.toggle("hidden", which !== "forgot");
    }

    document.addEventListener("click", (e) => {
      const t = e.target.closest ? e.target.closest("a,button") : null;
      if (!t) return;
      if (t.id === "reg-link") {
        e.preventDefault();
        showAuthForm("register");
        const el = $("#reg-email");
        if (el) el.focus();
      } else if (t.id === "forgot-link") {
        e.preventDefault();
        showAuthForm("forgot");
        const el = $("#forgot-email");
        if (el) el.focus();
      } else if (t.id === "reg-back" || t.id === "verify-back" || t.id === "forgot-back") {
        e.preventDefault();
        showAuthForm("login");
      }
    });

    // 找回密码提交（rant 2026-08-20：已注册（含未验证）账号 → 验证码 → 重置密码）
    if (forgotFormEl) {
      forgotFormEl.addEventListener("submit", async (e) => {
        e.preventDefault();
        const email = $("#forgot-email").value.trim();
        const code = $("#forgot-code").value.trim();
        const pw = $("#forgot-pass").value;
        const pw2 = $("#forgot-pass2").value;
        const errEl = $("#forgot-error");
        const showErr = (msg) => { errEl.textContent = msg; errEl.hidden = false; };
        const hideErr = () => { errEl.hidden = true; errEl.textContent = ""; };
        let firstErr = null;
        if (!email) { setFieldError($("#forgot-email"), T("login.err.email")); firstErr = firstErr || $("#forgot-email"); }
        else clearFieldError($("#forgot-email"));
        if (!code) { setFieldError($("#forgot-code"), T("verify.err.code")); firstErr = firstErr || $("#forgot-code"); }
        else clearFieldError($("#forgot-code"));
        if (pw.length < 8) { setFieldError($("#forgot-pass"), T("register.err.pass")); firstErr = firstErr || $("#forgot-pass"); }
        else clearFieldError($("#forgot-pass"));
        if (pw !== pw2) { setFieldError($("#forgot-pass2"), T("register.err.confirm")); firstErr = firstErr || $("#forgot-pass2"); }
        else clearFieldError($("#forgot-pass2"));
        if (firstErr) { firstErr.focus(); return; }
        hideErr();
        const btn = forgotFormEl.querySelector("button[type=submit]");
        const orig = btn.textContent;
        btn.disabled = true;
        try {
          // 先发验证码（未发送过则发；已发送会 429，忽略继续用已发的码）
          try { await api.post("/api/auth/forgot-password", { email }); }
          catch (fe) { /* 429 说明已有码，忽略 */ }
          const r = await api.post("/api/auth/reset-password", { email, code, new_password: pw });
          if (r && r.status === "ok") {
            toast(T("forgot.done"), "ok");
            showAuthForm("login");
            const le = $("#login-email");
            if (le) le.value = email;
          }
        } catch (err) {
          showErr((err && err.message) ? I18n.mapErr(err.message) : T("forgot.err.fail"));
        } finally {
          btn.disabled = false;
          btn.textContent = orig;
        }
      });
    }

    // 注册提交
    registerFormEl.addEventListener("submit", async (e) => {
      e.preventDefault();
      const email = $("#reg-email").value.trim();
      const name = $("#reg-name").value.trim();
      const pw = $("#reg-pass").value;
      const pw2 = $("#reg-pass2").value;
      let firstErr = null;
      const errEl = $("#reg-error");
      const showErr = (msg) => { errEl.textContent = msg; errEl.hidden = false; };
      const hideErr = () => { errEl.hidden = true; errEl.textContent = ""; };
      if (!email) { setFieldError($("#reg-email"), T("login.err.email")); firstErr = firstErr || $("#reg-email"); }
      else clearFieldError($("#reg-email"));
      if (!pw) { setFieldError($("#reg-pass"), T("register.err.pass")); firstErr = firstErr || $("#reg-pass"); }
      else clearFieldError($("#reg-pass"));
      if (pw !== pw2) { setFieldError($("#reg-pass2"), T("register.err.confirm")); firstErr = firstErr || $("#reg-pass2"); }
      else clearFieldError($("#reg-pass2"));
      if (firstErr) { firstErr.focus(); return; }
      hideErr();
      const btn = registerFormEl.querySelector('button[type="submit"]');
      if (btn) { btn.disabled = true; }
      try {
        const r = await api.post("/api/auth/register", { name, email, password: pw });
        // 成功 → 切到验证码页（dev 模式响应带 dev_code，提示到控制台/日志）
        $("#verify-email").value = email;
        showAuthForm("verify");
        if (r.dev_code) {
          toast(T("verify.devCode") + ": " + r.dev_code, "info");
        } else {
          toast(T("verify.sent"), "success");
        }
        $("#verify-code").focus();
      } catch (err) {
        if (err && err.status === 409) {
          setFieldError($("#reg-email"), T("register.err.taken"));
        } else {
          const m = (err && err.message) ? I18n.mapErr(err.message) : T("register.err.fail");
          showErr(m);
        }
      } finally {
        if (btn) { btn.disabled = false; }
      }
    });

    // 验证提交
    verifyFormEl.addEventListener("submit", async (e) => {
      e.preventDefault();
      const email = $("#verify-email").value.trim();
      const code = $("#verify-code").value.trim();
      const errEl = $("#verify-error");
      const showErr = (msg) => { errEl.textContent = msg; errEl.hidden = false; };
      const hideErr = () => { errEl.hidden = true; errEl.textContent = ""; };
      if (!code) { setFieldError($("#verify-code"), T("verify.err.code")); $("#verify-code").focus(); return; }
      clearFieldError($("#verify-code"));
      hideErr();
      const btn = verifyFormEl.querySelector('button[type="submit"]');
      if (btn) { btn.disabled = true; }
      try {
        await api.post("/api/auth/verify", { email, code });
        toast(T("verify.ok"), "success");
        showAuthForm("login");
        $("#login-email").value = email;
        $("#login-email").focus();
      } catch (err) {
        const m = (err && err.message) ? I18n.mapErr(err.message) : T("verify.err.fail");
        showErr(m);
      } finally {
        if (btn) { btn.disabled = false; }
      }
    });

    // 重发验证码
    $("#resend-btn").addEventListener("click", async () => {
      const email = $("#verify-email").value.trim();
      if (!email) return;
      const b = $("#resend-btn");
      const orig = b.textContent;
      b.disabled = true;
      b.textContent = T("verify.sending"); // 发送中反馈（rant 2026-08-21T14:08:03 补充验收）
      try {
        const r = await api.post("/api/auth/resend-code", { email });
        if (r.dev_code) { toast(T("verify.devCode") + ": " + r.dev_code, "info"); }
        else { toast(T("verify.sent"), "success"); }
      } catch (err) {
        const m = (err && err.message) ? I18n.mapErr(err.message) : T("verify.err.fail");
        toast(m, "error");
      } finally {
        setTimeout(() => { b.disabled = false; b.textContent = orig; }, 300);
      }
    });

    // 市场筛选（搜索防抖 ~150ms + 高亮 + 清空按钮，rant 18:06:09 D）
    // 厂商下拉在 renderMarketplace 内按数据源重建（登录=Live.models / 游客=data.js，零 mock rant 15:54:06）
    wireSearch($("#mk-search"), renderMarketplace);
    $("#mk-provider").addEventListener("change", renderMarketplace);
    $("#mk-sort").addEventListener("change", renderMarketplace);

    // 市场页：使用 / 消费（G4：聊天 Mock 扣小数点数并产生 consume 交易；游客需先登录 US-1）
    $("#mk-body").addEventListener("click", (e) => {
      // 行展开 / 收起（rant 20:39:30 F：仅展开当前行，点其它行自动收起）
      const ex = e.target.closest("[data-mk-expand]");
      if (ex) {
        const id = Number(ex.dataset.mkExpand);
        mkExpanded = mkExpanded === id ? null : id;
        renderMarketplace();
        return;
      }
      const b = e.target.closest("[data-use-model]");
      if (b) {
        if (isGuest) { toast(T("chat.login.need"), "error"); return; }
        consumeModel(Number(b.dataset.useModel));
        return;
      }
      // 空状态：清除筛选
      if (e.target.closest("[data-mk-clear-filters]")) {
        resetSearch($("#mk-search"));
        $("#mk-provider").value = "";
        $("#mk-sort").value = "default";
        renderMarketplace();
      }
    });
    // 最近使用 chips（rant 20:46:57 D：点击直接使用 / 清空）
    $("#mk-recent").addEventListener("click", (e) => {
      if (e.target.closest("[data-mk-recent-clear]")) {
        saveRecentIds([]);
        renderRecent();
        return;
      }
      const c = e.target.closest("[data-recent-model]");
      if (c) {
        if (isGuest) { toast(T("chat.login.need"), "error"); return; }
        openChat(Number(c.dataset.recentModel));
      }
    });
    $("#chat-send").addEventListener("click", sendChat);
    $("#chat-close").addEventListener("click", closeChat);
    $("#chat-input").addEventListener("keydown", (e) => { if (e.key === "Enter") sendChat(); });
    $("#chat-modal").addEventListener("click", (e) => { if (e.target === $("#chat-modal")) closeChat(); });

    // 运营视图 Tabs（P2-C：运行概览 / 成员充值）
    $$("#ops-tabs .tab").forEach((b) => b.addEventListener("click", () => {
      $$("#ops-tabs .tab").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      renderOps();
    }));

    // 平台运营者（G1 / US-运营2）：搜索定位用户 + 充值（行内编辑，永久有效点数，产生交易记录）
    wireSearch($("#ops-search"), renderOps);
    $("#ops-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-ops-topup]");
      if (!b) return;
      const src = Live.opsUsers || [];
      const u = src.find((x) => x.id === Number(b.dataset.opsTopup));
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
        const plan = (Live.plans || D.PLANS).find((pl) => pl.id === planId);
        const quota = Number($("#sf-quota").value || 0);
        const key = $("#sf-key").value.trim();
        const note = $("#sf-note").value.trim();
        let firstErr = null;
        if (!key) { setFieldError($("#sf-key"), T("share.err.key")); firstErr = firstErr || $("#sf-key"); }
        else clearFieldError($("#sf-key"));
        if (!plan || !model || quota <= 0) {
          setFieldError($("#sf-quota"), T("share.err.plan"));
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
        const payload = {
          provider: plan.provider,
          plan: plan.id,
          model,
          key,
          quota,
          available,
          note,
        };
        const afterOk = () => {
          e.target.reset();
          const p = $("#sf-provider"); p.value = ""; p.dispatchEvent(new Event("change"));
          $("#sf-quota").value = 5000;
          hideShareForm();
          const label = provLabel(plan.provider) + " · " + plan.name;
          toast(T("share.list.ok", { label: label, model: model, price: D.fmt(price) }), "success");
        };
        if (!loggedIn()) {
          toast(T("chat.login.need"), "error");
          return;
        }
        // P2-B：真实上架（共享管理仅登录可达，零 mock rant 15:54:06）
        api.post("/api/sharings", payload).then(async () => {
          await loadSharing();
          if (activeView === "dashboard") renderDashboard();
          afterOk();
        }).catch((err) => {
          toast((err && err.message) ? I18n.mapErr(err.message) : T("share.list.fail"), "error");
        });
      };
      withLoading(submitBtn, done);
    });

    // 共享列表操作（事件委托：暂停/恢复/重新上架 + 删除[行内二次确认] + 空状态上架）
    $("#share-body").addEventListener("click", (e) => {
      const b = e.target.closest("[data-share-toggle]");
      if (b) { toggleSharing(Number(b.dataset.shareToggle)); return; }
      const d = e.target.closest("[data-share-delete]");
      if (d) { confirmInline(d, () => deleteSharing(Number(d.dataset.shareDelete)), T("share.del.confirm")); return; }
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

    // 交易 Tab（P2-B：切 tab 重新拉后端过滤数据）
    $$("#tx-tabs .tab").forEach((b) => b.addEventListener("click", () => { txTab = b.dataset.txTab; txTable.page = 1; renderTransactions(); if (loggedIn()) loadTransactions(); }));
    $("#tx-export-btn").addEventListener("click", exportTxCsv); // 导出 CSV（rant 20:46:57 E）

    // 表格键盘导航（rant 20:46:57 F）：点击行 → 激活高亮，之后 ↑/↓/Enter/Esc 可用
    KBD_TABLE_IDS.forEach((id) => {
      const c = document.getElementById(id);
      if (!c) return;
      c.addEventListener("click", (e) => {
        const tr = e.target.closest ? e.target.closest("tr") : null;
        if (!tr || (tr.classList && tr.classList.contains("mk-detail"))) return;
        kbdSet(c, kbdRows(c).indexOf(tr));
      });
    });

    // API Key 生成（行内编辑；列表展示脱敏、复制给完整 id）
    $("#new-api-key-btn").addEventListener("click", openNewKeyInline);
    $("#ak-new-ok").addEventListener("click", commitNewKey);
    $("#ak-new-cancel").addEventListener("click", closeNewKeyInline);
    $("#ak-new-name").addEventListener("keydown", (e) => {
      if (e.key === "Enter") commitNewKey();
      else if (e.key === "Escape") closeNewKeyInline();
    });

    // API Key 搜索 + 行内操作（复制 / 改名 / 删除[行内二次确认]）
    wireSearch($("#ak-search"), renderSettings);

    // 接入端点复制（rant 2026-08-17T20:44:18）
    document.querySelectorAll("[data-ep-copy]").forEach((b) =>
      b.addEventListener("click", () => copyEndpoint(Number(b.dataset.epCopy))));

    $("#api-keys").addEventListener("click", (e) => {
      const cp = e.target.closest("[data-key-copy]");
      if (cp) { copyKey(Number(cp.dataset.keyCopy)); return; }
      const rn = e.target.closest("[data-key-rename]");
      if (rn) { renameKey(Number(rn.dataset.keyRename)); return; }
      const dl = e.target.closest("[data-key-del]");
      if (dl) { confirmInline(dl, () => deleteKey(Number(dl.dataset.keyDel)), T("settings.ak.del.confirm")); return; }
      if (e.target.closest("[data-new-key]")) openNewKeyInline();
    });

    // 管理台 Tabs
    $$("#admin-tabs .tab").forEach((b) => b.addEventListener("click", () => {
      $$("#admin-tabs .tab").forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      renderAdmin();
    }));

    // 成员充值（管理台）：行内编辑（替代原生输入弹窗，Enter 确认 / Esc 取消）
    // 零 mock（rant 15:54:06）：仅真实成员（/api/admin/users）→ POST /api/admin/credits
    $("#emp-body").addEventListener("click", (e) => {
      const dd = e.target.closest("[data-emp-dept]");
      if (dd) { editEmpDept(Number(dd.dataset.empDept)); return; }
      const b = e.target.closest("[data-emp-topup]");
      if (!b) return;
      const liveEmp = Live.adminUsers ? Live.adminUsers[Number(b.dataset.empTopup)] : null;
      if (!liveEmp) return;
      const row = b.closest("tr");
      if (!row) return;
      const cell = row.children[row.children.length - 1];
      inlineForm(cell, {
        value: "5000",
        placeholder: T("admin.emp.topup.ph"),
        type: "number",
        width: "120px",
        validate: (raw) => {
          const amt = Number(raw);
          return (!raw || !Number.isInteger(amt) || amt <= 0) ? T("admin.emp.topup.err") : null;
        },
        onSubmit: (raw) => {
          const amt = Number(raw);
          api.post("/api/admin/credits", { user_id: liveEmp.id, amount: amt, note: "admin recharge" })
            .then(async () => {
              await loadAdmin();
              toast(T("admin.emp.topup.ok", { name: liveEmp.name || liveEmp.email, amt: D.fmt(amt) }), "success");
            })
            .catch((err) => toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.emp.topup.fail"), "error"));
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
    wireSearch($("#od-search"), renderAdmin);

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
      if (dl) { confirmInline(dl, () => deleteDept(Number(dl.dataset.deptDel)), T("admin.org.del.confirm")); return; }
      if (e.target.closest("[data-dept-clear-search]")) {
        resetSearch($("#od-search"));
        renderAdmin();
      }
    });

    // 模型管理（rant 2026-08-19T20:40:29）：搜索 / 添加 / 编辑 / 删除 / 表单
    wireSearch($("#model-search"), renderAdmin);

    $("#add-model-btn").addEventListener("click", () => openModelForm(null));
    $("#model-confirm").addEventListener("click", (e) => withLoading(e.currentTarget, confirmModel));
    $("#model-cancel").addEventListener("click", () => { $("#model-form-card").hidden = true; });
    $("#model-form-provider").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("#model-confirm").click(); } });
    $("#model-form-model").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("#model-confirm").click(); } });
    $("#model-form-card").addEventListener("keydown", (e) => { if (e.key === "Escape") { $("#model-form-card").hidden = true; } });

    $("#model-body").addEventListener("click", (e) => {
      const em = e.target.closest("[data-model-edit]");
      if (em) { editModelRow(Number(em.dataset.modelEdit)); return; }
      const dl = e.target.closest("[data-model-del]");
      if (dl) { confirmInline(dl, () => deleteModel(Number(dl.dataset.modelDel)), T("admin.models.del.confirm")); return; }
    });
  }

  /* ---------------- 初始化 ---------------- */

  document.addEventListener("DOMContentLoaded", () => {
    document.title = "AITokenPool"; // 默认标题（rant 18:06:09 F：无视图时回「AITokenPool」）
    // URL hash 路由（rant 20:39:30 A）：加载时记录 hash 视图（登录后恢复）；前进/后退触发 hashchange
    pendingHashView = viewFromHash();
    window.addEventListener("hashchange", () => {
      const id = viewFromHash();
      // 非法 hash → 回仪表盘但不重写 URL（避免 pushState 新增历史条目、后退需两次）
      if (id && id !== activeView) switchView(id, { sync: hashIsValid() });
    });
    renderNav();
    bindEvents();
    // 登录态加载失败的空态重试按钮（rant 2026-08-19T15:48:17 / 15:54:06：各视图 loadError* 的 data-live-retry 委托）
    bindLiveRetry("dash-sharings", () => loadDashboard());
    bindLiveRetry("share-body", () => loadSharing());
    bindLiveRetry("mk-body", () => loadMarketplace());
    bindLiveRetry("tx-table", () => loadTransactions());
    bindLiveRetry("api-keys", () => loadApiKeys());
    bindLiveRetry("emp-body", () => loadAdmin());
    bindLiveRetry("dept-body", () => loadAdmin());
    bindLiveRetry("usage-model", () => loadAdmin());
    bindLiveRetry("usage-emp", () => loadAdmin());
    bindLiveRetry("usage-dept", () => loadAdmin());
    bindLiveRetry("raise-requests", () => loadAdmin());
    bindLiveRetry("ops-stats", () => loadOps());
    bindLiveRetry("ops-body", () => loadOps());
    renderView("dashboard");
    $("#side-balance").textContent = D.fmt(D.USER.balance);

    // P2-A 会话恢复：已有 token → 拉 /api/me + /api/wallet 直接进 app；401 自动清 token 回登录页
    (async () => {
      if (!api.getToken()) return;
      try {
        await loadSession();
        enterApp();
      } catch (e) {
        // 401 已由 api.js 清 token；其余错误保持登录页并提示
        if (!(e && e.status === 401)) toast(T("login.session.fail"), "error");
      }
    })();

    // 主题（rant 18:06:09 B）：localStorage 记忆，首次加载尊重 prefers-color-scheme
    const savedTheme = (() => { try { return localStorage.getItem("atp-theme"); } catch (e) { return null; } })();
    const initialTheme = savedTheme === "light" || savedTheme === "dark"
      ? savedTheme
      : (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
    document.documentElement.dataset.theme = initialTheme;
    // 记住我（rant 20:39:30 G）：还原上次勾选状态
    try { $("#login-remember").checked = localStorage.getItem("atp-remember") === "1"; } catch (e) { /* 隐私模式忽略 */ }
    // 表格密度（rant 20:46:57 C）：还原上次选择
    applyDensity(getDensity());
    document.querySelectorAll('input[name="density"]').forEach((r) => {
      r.checked = r.value === getDensity();
      r.addEventListener("change", () => applyDensity(r.value));
    });
    // 界面语言（rant 2026-08-18T20:49:22 i18n）：下拉切换 + localStorage 记忆 + 即时重渲染
    const langSel = $("#prefs-lang");
    if (langSel) {
      langSel.value = I18n.getLang();
      langSel.addEventListener("change", () => I18n.setLang(langSel.value));
    }
    document.addEventListener("atp:langchange", () => {
      if (langSel) langSel.value = I18n.getLang();
      renderNav();
      if (activeView) renderView(activeView);
      document.title = (VIEW_TITLE[activeView] ? T(VIEW_TITLE[activeView]) + " · AITokenPool" : "AITokenPool");
      if (tourStep >= 0) renderTourStep(); // 引导中的按钮/文案随语言更新
    });

    $("#theme-toggle").addEventListener("click", () => {
      const next = document.documentElement.dataset.theme === "light" ? "dark" : "light";
      document.documentElement.dataset.theme = next;
      try { localStorage.setItem("atp-theme", next); } catch (e) { /* 隐私模式忽略 */ }
      toast(T("theme.switched", { theme: next === "light" ? T("theme.light") : T("theme.dark") }), "info");
    });

    // 全局快捷键（rant 16:57:17 D）：/ 聚焦市场搜索；数字 1-7 切换侧边栏视图；Esc 关闭行内新建 key
    // rant 20:39:30 E：? / Shift+/ 开合快捷键帮助面板（Esc 优先关帮助）
    // rant 20:46:57 A：引导中 Esc 优先关引导
    document.addEventListener("keydown", (e) => {
      const t = e.target;
      const typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
      const tourOpen = tourStep >= 0;
      if (e.key === "Escape" && tourOpen) { closeTour(); return; }
      const helpOpen = !$("#help-panel").classList.contains("hidden");
      if (e.key === "Escape" && helpOpen) { toggleHelp(false); return; }
      if (e.key === "Escape" && !$("#ak-new-inline").hidden) { closeNewKeyInline(); return; }
      if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
      // 表格键盘导航（rant 20:46:57 F）：↑/↓ 行高亮，Enter 主操作，Esc 清除（无高亮时 Esc 落到后续逻辑）
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const c = kbdContainerFrom(t);
        if (!c) return;
        e.preventDefault();
        kbdMove(e.key === "ArrowDown" ? 1 : -1, c);
        return;
      }
      if (e.key === "Enter") { kbdEnter(); return; }
      if (e.key === "Escape" && kbd.c) { kbdClear(); return; }
      if (e.key === "?") { toggleHelp(); return; }
      if (e.key === "/") {
        e.preventDefault();
        $("#mk-search").focus();
        return;
      }
      if (e.key >= "1" && e.key <= "7") {
        const item = NAV_ORDER[Number(e.key) - 1];
        if (item && (!item.role || D.USER.role === item.role)) switchView(item.id);
      }
    });
    $("#help-close").addEventListener("click", () => toggleHelp(false));

    // 首次引导 tour 事件（rant 20:46:57 A）：点浮层外关闭；气泡内 跳过/上一步/下一步/完成；设置页重放
    $("#tour-overlay").addEventListener("click", closeTour);
    $("#tour-pop").addEventListener("click", (e) => {
      const b = e.target.closest("[data-tour-action]");
      if (!b) return;
      const act = b.dataset.tourAction;
      if (act === "skip") { closeTour(); return; }
      if (act === "prev" && tourStep > 0) { tourStep--; renderTourStep(); return; }
      if (act === "next") {
        if (tourStep < TOUR_STEPS.length - 1) { tourStep++; renderTourStep(); }
        else closeTour();
      }
    });
    $("#tour-replay-btn").addEventListener("click", () => { startTour(); });
  });
})();
