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
  // 交易类型筛选**只有一份状态**：`txTable.filters.type`（见 txTypeFilter()，原 `txTab` 已删）
  let txRange = "24h"; // 交易时间段快捷范围：24h / 7d / 30d / all / custom（默认最近 24 小时，rant 2026-08-22T10:50:00）
  let txCustomStart = ""; // 自定义开始（datetime-local 值，本地时区）
  let txCustomEnd = ""; // 自定义结束
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
    ["1–8", "help.k2"],
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

  /* --- 行内表单卡片的 Enter 提交（C2127） --- */

  // 哪些输入控件按 Enter 等于「提交」：文本类。非文本类（勾选框 / 单选框 / 下拉 / 文件 /
  // 范围 / 颜色 / 隐藏域）按 Enter 不提交 —— 真 `<form>` 的隐式提交也是这个规则，
  // 这里刻意对齐它，别自己发明一套。
  const NON_TEXT_INPUT_TYPES = ["checkbox", "radio", "button", "submit", "reset", "file", "range", "color", "hidden"];

  // 把「Enter 提交」挂到**容器**上，而不是逐字段登记（C2127）。
  //
  // 这些卡片是 `<div class="form">` / `<span class="inline-edit">` 而非真 `<form>`
  // （真表单见 `#share-form`：`type=submit` 按钮让浏览器自己实现隐式提交，全字段免费），
  // 所以 Enter 得自己实现。逐字段 `$("#某字段").addEventListener("keydown", …)` 等于把
  // 「哪些控件能提交」抄成一份**名册** —— 而名册不会随控件增长：模型表单曾有 10 个可输入
  // 控件、只登记了 2 个，另外 8 个（输入价 / 输出价 / 上下文窗口 …）按 Enter 毫无反应，
  // 用户只会以为界面卡住；同一页的部门表单却每个字段都登记了（2/2）⇒ 漂移不是取舍。
  //
  // 容器级委托挂在事件冒泡上 ⇒ 卡片里**当前和以后**的文本控件都自动生效，名册消失。
  function wireEnterSubmit(card, confirmSel) {
    if (!card) return;
    card.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      const t = e.target;
      // 只认「往里打字」的控件：焦点在按钮/勾选框/下拉上按 Enter 不提交
      // （否则在「取消」上按 Enter 会同时触发取消与提交）。
      if (!t || t.tagName !== "INPUT") return;
      if (NON_TEXT_INPUT_TYPES.includes((t.type || "text").toLowerCase())) return;
      const btn = $(confirmSel);
      if (!btn || btn.disabled) return;
      e.preventDefault();
      btn.click(); // 走确认按钮自己的监听器（忙碌态、校验、请求都在那一处，不重复实现）
    });
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
    const box = input.closest(".search");
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
    const box = input.closest(".search");
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

  // 状态徽标（原型 .pill-ok/-warn/-danger/-muted/-accent，rant 2026-09-11T16:23:43 第三节）
  // labels[status].cls 取语义色名（ok/warn/danger/dim/accent）→ 映射到原型 pill 类
  const PILL_CLS = { ok: "pill-ok", warn: "pill-warn", danger: "pill-danger", dim: "pill-muted", accent: "pill-accent" };
  function badge(status, labels) {
    const l = labels[status];
    const text = l && typeof l.text === "function" ? l.text() : (l ? l.text : status);
    return '<span class="pill ' + (PILL_CLS[l ? l.cls : "dim"] || "pill-muted") + '">' + esc(text) + "</span>";
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

  // Plan 提示：按量/订阅（来自 PLANS；登录后为 /api/plans）
  // 注：曾拼接 ` · {note}`（data.js 的端点说明），但 /api/plans 从不返回 note
  //（config [[plans]] 与 Plan 结构体均无该字段）⇒ 登录态（唯一可达路径）恒为 undefined，
  // 该后缀永不显示 ⇒ 已移除读点与 i18n 键 `share.plan.note`（同 44a3040 清 keyPrefix 的先例）。
  function showPlanHint(planId) {
    const el = $("#sf-plan-hint");
    if (!el) return;
    const plans = Live.plans || D.PLANS;
    const pl = plans.find((x) => x.id === planId);
    if (!pl) { el.textContent = ""; return; }
    el.textContent = pl.type === "paygo" ? T("share.plan.paygo") : T("share.plan.sub");
  }

  // Plan 显示名（C2133）：config 写了 name 就用它的原文，否则按 type 取语言包。
  // 后端只回传 config 原值（未配置 = 空串，语言中性）——显示文案归语言包，否则
  // `en` 界面会把响应数据字段里的后端自造中文原样印出来（mapErr 只认 `error` 字段）。
  function planLabel(pl) {
    if (pl.name) return pl.name;
    if (pl.type === "paygo") return T("share.planName.paygo");
    if (pl.type === "token") return T("share.planName.token");
    if (pl.type === "coding") return T("share.planName.coding");
    return pl.id;
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

  // 侧边栏视图顺序（rant 16:57:17 D：数字键 1..N 切换对应视图，N = 本数组长度，title 提示快捷键）
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
    // 游客导航只列市场，但必须**引用登记表 NAV_ORDER 里的同一个对象**：角标由它在那个数组中的
    // 下标算出、数字键处理器也按同一数组取项（`NAV_ORDER[Number(e.key) - 1]`）。手搓一个同形
    // 字面量 ⇒ `indexOf` 恒 -1 ⇒ 角标 0（死键、`NAV_ORDER[-1]` 落空），而真正能打开市场的键
    // （2）游客永远看不到；两边都「看起来正常」，只有按下去才知道。
    const groups = isGuest
      ? [{ g: "nav.guest", items: NAV_ORDER.filter((it) => GUEST_VIEWS.includes(it.id)) }]
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
        const short = NAV_ORDER.indexOf(item) + 1;
        b.title = T("nav.shortcut", { n: short, label: T(item.label) });
        b.innerHTML = '<span class="ico">' + (ICONS[item.icon] || "") + '</span><span class="label">' + esc(T(item.label)) + "</span>" +
          (item.role ? "" : '<span class="nav-key">' + short + "</span>");
        if (item.role) {
          const tag = document.createElement("span");
          tag.className = "nav-tag";
          tag.textContent = item.role === "ops" ? T("nav.tag.ops") : T("nav.tag.admin");
          b.appendChild(tag);
        }
        // 模型市场 badge：模型数量（rant 2026-09-11T16:23:43）——与列表同源：登录态 /api/models，游客用兜底市场。
        // 登录态目录缺席时**不显示数字**：上架表单的价格镜像（MODELS）不是市场目录，两者行数与顺序都不同
        if (item.id === "marketplace") {
          // 与列表同源：marketRows() 已按会话状态决定数据源（登录态目录缺席 ⇒ 0，不显示数字）
          const n = (marketRows() || []).length;
          if (n > 0) {
            const bg = document.createElement("span");
            bg.className = "nav-count";
            bg.textContent = String(n);
            b.appendChild(bg);
          }
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
    else if (id === "wallet") { renderWallet(); if (loggedIn()) loadWallet(); }
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
      // C2130：读仪表盘**自己的**槽，而不是交易视图的载荷缓存 —— 见 Live.tradeCount 的注释
      tradeCount = typeof Live.tradeCount === "number" ? Live.tradeCount : 0;
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

    renderDashTrend();

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
      setLiveError($("#dash-sharings"), loadErrorHtml(T("dash.loadFail"), T("err.loadFail")), () => loadDashboard());
    }
    renderMonthChanges();
  }

  // 仪表盘「近 14 天消耗与收益」双色柱图（rant 2026-09-11T16:23:43 第 3 节：仪表盘新增
  // 双色趋势图，消费=accent / 共享收益=ok，含 .legend 图例，柱高按当日 max 归一，最小高度 2%）。
  // 零 mock：数据源为 /api/transactions/trend（income/expense 已按日聚合，口径与交易页一致），
  // 登录态失败 → 空态文案，绝不回落 D. 静态序列（rant 15:54:06）。
  //
  // 补齐空日（DASH_TREND_DAYS）：后端 GROUP BY 只返回「有交易」的日桶，无交易的日期直接缺行。
  // 原型是固定 14 列柱图，缺行会让柱子左右移位、横轴节奏错乱（今天可能不在最右）。
  // 故按请求窗口铺满 14 天，缺数据的日子补 0（柱高 min 2%，tooltip 显示 0），保持固定节奏。
  function dashTrendDays(buckets) {
    const byKey = new Map();
    buckets.forEach((b) => {
      const d = new Date(b.t);
      if (!isNaN(d.getTime())) byKey.set(d.toISOString().slice(0, 10), b);
    });
    // 窗口与 loadDashboard 请求的 start（now - 13d）对齐，锚点取 UTC 日（后端 time 亦是 UTC）
    const now = new Date();
    const todayUtc = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
    const days = [];
    for (let i = DASH_TREND_DAYS - 1; i >= 0; i--) {
      const t = new Date(todayUtc - i * 864e5).toISOString().slice(0, 10);
      days.push(byKey.get(t) || { t: t + "T00:00:00Z", income: 0, expense: 0, tokens: 0, count: 0 });
    }
    return days;
  }

  function renderDashTrend() {
    const el = $("#dash-trend");
    if (!el) return;
    const tr = (Live.dashboardTrend && Array.isArray(Live.dashboardTrend.buckets)) ? Live.dashboardTrend : null;
    const raw = tr ? tr.buckets : [];
    const logged = loggedIn();
    if (!logged) {
      el.innerHTML = '<div class="empty compact">' + esc(T("dash.trend.guest")) + "</div>";
      return;
    }
    // 窗口内一笔交易都没有 → 空态（保留零 mock 语义，不去画一排 0 柱）
    if (!raw.length) {
      el.innerHTML = '<div class="empty compact">' + esc(Live.dashboardTrend === null ? T("dash.trend.fail") : T("dash.trend.empty")) + "</div>";
      return;
    }
    const buckets = dashTrendDays(raw);
    // 柱高按当日 max(消费, 收益) 归一，最小高度 2%（原型规则）
    const max = Math.max(1, ...buckets.map((b) => Math.max(b.expense || 0, b.income || 0)));
    el.innerHTML = buckets.map((b) => {
      const c = b.expense || 0, e = b.income || 0;
      const day = bucketLabel(b.t, "day");
      const h = (v) => Math.max(2, (v / max) * 100).toFixed(1);
      const tip = day + " " + T("dash.trend.consume") + " " + D.fmt(c) + " / " + T("dash.trend.earn") + " " + D.fmt(e);
      return '<div class="trend-col" title="' + esc(tip) + '"><div class="trend-pair">' +
        '<div class="trend-bar consume" style="height:' + h(c) + '%"></div>' +
        '<div class="trend-bar earn" style="height:' + h(e) + '%"></div>' +
        '</div><span class="trend-x">' + esc(day) + "</span></div>";
    }).join("");
  }

  // P2-B：拉取仪表盘所需数据（wallet + dashboard + sharings + 交易数）
  async function loadDashboard() {
    if (!loggedIn()) return;
    try { await refreshWallet(); } catch (e) { /* 降级 */ }
    await refreshDashboard();
    // 近 14 天双色趋势（rant 2026-09-11T16:23:43 第 3 节）：复用交易页趋势接口，
    // 按日聚合 income/expense，与列表同口径；失败 → null（renderDashTrend 显示空态，不 mock）。
    // start 取「今天 UTC 零点 - 13 天」而非 now-13d：与后端 strftime('%Y-%m-%d', time)（UTC 日桶）
    // 及 renderDashTrend 的补齐锚点三者对齐，否则跨零点时首桶会被截掉（窗口只剩 13 天）。
    try {
      const now = new Date();
      const todayUtc = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
      const start = new Date(todayUtc - (DASH_TREND_DAYS - 1) * 864e5).toISOString();
      Live.dashboardTrend = await api.get("/api/transactions/trend?type=all&bucket=day&start=" + encodeURIComponent(start));
    } catch (e) { Live.dashboardTrend = null; }
    // 交易数统计（dash.trades）：拉 1 条取 total（零 mock，rant 2026-08-19T15:54:06）
    //
    // C2130：这是**另一种查询**的载荷（`page_size=1`，只为读 `total`），只能进仪表盘**自己的**槽。
    // 它曾写进 `Live.transactions` —— 那是**交易视图自己的**载荷缓存，由 `loadTransactions` 写入，
    // 并由同一个函数写下它的有效性证据（`txTable.loadedPage/loadedPageSize/loadedFilterSig`）。
    // 换了写者，证据就与缓存内容脱钩：`renderTransactions` 的守卫比的正是那三项，
    // 于是**放行**仪表盘的载荷 —— 再入交易页时表格只剩 1 行（page_size=1）、汇总卡挂着
    // 「当前筛选」却显示全时段数字（仪表盘那次请求不带时间范围）、趋势卡谎报「加载失败」
    // （`trend` 只有 `loadTransactions` 会挂上）。仪表盘只要那个**数字**。
    try {
      const page = await api.get("/api/transactions?page=1&page_size=1");
      Live.tradeCount = typeof page.total === "number" ? page.total : 0;
    } catch (e) { Live.tradeCount = null; }
    // rant 2026-08-19T15:48:17：仪表盘「我的共享」此前从不拉 sharings → 登录态恒显示 D.SHARINGS mock；
    // 现在拉真实数据；失败置 null → renderDashboard 走空态 + 重试（mock 仅游客）
    try {
      Live.sharings = await api.get("/api/sharings");
    } catch (e) { Live.sharings = null; }
    renderDashboard();
  }

  function stat(label, value, sub, cls) {
    return '<div class="stat-card' + (cls ? " " + cls : "") + '"><div class="label">' + esc(label) +
      '</div><div class="value">' + value + "</div><div class='sub'>" + esc(sub) + "</div></div>";
  }

  /* --- 模型市场 --- */

  // 市场数据源（C2140）：数据源由**会话状态**决定，不由「目录到没到」决定（data.js 契约，
  // rant 2026-08-19T15:54:06）。登录态只认 /api/models —— 目录缺席（失败/超时/还没到）时返回
  // `null`，交给各视图自己的降级态（加载失败 + 重试），**绝不**拿 data.js 的游客表冒充自己的数据；
  // 游客才用兜底表。市场面的每个消费者都走这两个 helper —— 表只在这一处读，判别式只在这一处写。
  function marketRows() { return Live.models ? modelsToView(Live.models) : (loggedIn() ? null : D.MARKET); }
  function marketProviders() { return Live.models ? [...new Set(Live.models.map((m) => m.provider))] : (loggedIn() ? [] : D.PROVIDERS); }

  // 最近使用（rant 20:46:57 D：localStorage atp-recent-models 最近 5 个去重，复用 .chip，点击直接使用）
  const RECENT_MAX = 5;
  const RECENT_KEY = "atp-recent-models";
  function getRecentKeys() {
    try {
      const arr = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
      // 只认身份串（provider/model）。旧版本存的是**下标**（数字）—— 它无法被诚实地还原成
      // 某个模型（数组顺序一变、换一张表，同一个数字就是另一个模型），故按空处理、一次性丢弃。
      return Array.isArray(arr) ? arr.filter((x) => typeof x === "string" && x.indexOf("/") > 0) : [];
    } catch (e) { return []; } // 旧数据/隐私模式：按空处理
  }
  function saveRecentKeys(keys) {
    try { localStorage.setItem(RECENT_KEY, JSON.stringify(keys.slice(0, RECENT_MAX))); } catch (e) { /* 隐私模式忽略 */ }
  }
  function markRecentUsed(key) {
    const keys = getRecentKeys().filter((x) => x !== key); // 去重：已存在则先移除
    keys.unshift(key);                                     // 最新使用放最前
    saveRecentKeys(keys);
  }
  function renderRecent() {
    const wrap = $("#mk-recent-chips");
    const chips = getRecentKeys().map((key) => {
      // 登录态目录缺席 ⇒ 没有芯片（点开也只会提示加载失败），绝不拿游客市场的模型冒充
      const m = (marketRows() || []).find((x) => modelKey(x) === key);
      return m ? '<button type="button" class="chip" data-recent-model="' + esc(modelKey(m)) + '" title="' + esc(m.provider) + " · " + T("mk.recent.use") + '">' + esc(m.model) + "</button>" : null;
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

    // 厂商筛选下拉：与列表同源（marketProviders()）——登录态只用活目录的厂商，
    // 目录缺席时下拉里只有「全部」（放一个本部署没有的厂商，只会筛出 0 行）
    const provEl = $("#mk-provider");
    const provSrc = Live.models ? "live" : (loggedIn() ? "none" : "mock");
    if (provEl && provEl.dataset.provSource !== provSrc) {
      const providers = marketProviders();
      provEl.dataset.provSource = provSrc;
      const cur = provEl.value;
      provEl.innerHTML = '<option value="">' + T("mk.provider.all") + "</option>" +
        providers.map((p) => '<option value="' + p + '">' + p + "</option>").join("");
      if (cur && providers.includes(cur)) provEl.value = cur;
    }

    // P2-B：登录 → /api/models 真实列表；游客 → data.js mock（mock 仅游客，rant 15:54:06）；
    // 登录态加载失败 → 空态 + 重试（loadErrorRow），绝不 fallback D.MARKET
    let list = marketRows();
    if (!list) {
      $("#mk-count").textContent = T("cnt.on", { n: 0 });
      setLiveError($("#mk-body"), loadErrorRow(7, T("mk.loadFail"), T("err.loadFail")), () => loadMarketplace());
      pulseTbody($("#mk-body"));
      renderRecent();
      return;
    }
    list = list.filter((m) =>
      (!q || m.model.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q)) &&
      (!prov || m.provider === prov)
    );
    // 可用性筛选（rant 2026-09-11T16:23:43 第 4 节：all / 仅可用）
    const avail = $("#mk-avail") ? $("#mk-avail").value : "all";
    if (avail === "yes") list = list.filter((m) => m.avail);
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
      "<tr><td data-label='" + T('mk.col.providerModel') + "'>" +
      '<div class="provider-cell">' +
      '<button type="button" class="row-expand" data-mk-expand="' + esc(modelKey(m)) + '" title="' + (mkExpanded === modelKey(m) ? T("mk.collapse") : T("mk.expand")) + '">' + (mkExpanded === modelKey(m) ? "−" : "+") + "</button>" +
      '<span class="dot' + (m.avail ? "" : " muted") + '"></span>' +
      '<span><span class="muted" style="font-size:11.5px;display:block">' + hl(m.provider, rawQ) + "</span>" +
      '<span class="model-name">' + hl(m.model, rawQ) + "</span></span></div></td>" +
      '<td class="num" data-label="' + T("mk.col.in") + '">' + D.fmt(m.in) + " " + T("common.points") +
      (m.peak ? ' <span class="tag tag-accent" title="' + esc(T("mk.peak.title", { n: m.peakMult })) + '">' + esc(T("mk.peak.badge", { n: m.peakMult })) + "</span>" : "") + "</td>" +
      '<td class="num" data-label="' + T("mk.col.out") + '">' + D.fmt(m.out) + " " + T("common.points") + "</td>" +
      '<td class="num" data-label="' + T("mk.col.ctx") + '">' + D.ctxFmt(m.ctx) + "</td>" +
      // 能力标签（rant 第 4 节：旗舰/推理 tag-accent，读图另加 tag）——仅渲染后端真实字段
      '<td data-label="' + T("mk.col.caps") + '">' + capabilityTags(m) + "</td>" +
      // 可用性 pill（rant 第 4 节：keys>=2 可用·N key / keys==1 紧张 / 无计数但标着可用 → 可用 / 无 key）
      // —— 有计数用计数，没计数用 `avail`（游客兜底表只有布尔，见 availPill 的注释）
      "<td data-label='" + T('mk.col.avail') + "'>" + availPill(m) + "</td>" +
      "<td data-label='" + T('mk.col.action') + "'><button class='btn btn-primary btn-sm' data-use-model='" + esc(modelKey(m)) + "'" + (m.avail ? "" : " disabled") + ">" + T("mk.use") + "</button>" +
      // 零 mock：成功率后端暂无字段 → 仅当有真实值时展示（multi/success 已从 data.js 移除）
      (m.success != null ? "<div class='muted' style='margin-top:4px;font-size:12px'>" + T("mk.success", { p: m.success }) + "</div>" : "") + "</td></tr>" +
      (mkExpanded === modelKey(m) ? '<tr class="mk-detail"><td colspan="7">' + mkDetailHtml(m) + "</td></tr>" : "")
    ).join("") : emptyRow(7, T("mk.empty"), T("mk.empty.sub"),
      '<button type="button" class="btn btn-ghost" data-mk-clear-filters>' + T("mk.clearFilters") + "</button>"));
    pulseTbody($("#mk-body"));
    renderRecent(); // 最近使用 chips（rant 20:46:57 D）
  }

  // 能力标签（rant 2026-09-11T16:23:43 第 4 节）：只用后端真实字段渲染，缺字段则不出标签
  // 读图 = vision；多 key = 多路可用；高峰计价 = peak（tag-accent 强调）
  function capabilityTags(m) {
    const tags = [];
    if (m.vision) tags.push('<span class="tag" title="' + esc(T("mk.cap.vision.title")) + '">' + esc(T("mk.cap.vision")) + "</span>");
    if (m.multi) tags.push('<span class="tag" title="' + esc(T("mk.multi")) + '">' + esc(T("mk.multi")) + "</span>");
    if (m.peak) tags.push('<span class="tag tag-accent" title="' + esc(T("mk.peak.title", { n: m.peakMult })) + '">' + esc(T("mk.cap.peak")) + "</span>");
    return tags.length ? tags.join(" ") : '<span class="muted">—</span>';
  }

  // 可用性 pill（rant 第 4 节）：key 数三态；无 key → muted 且「使用」按钮禁用
  //
  // ⚠️ 一个市场行的「可用」只有**一个**事实：`avail`（绿点 / 「使用」按钮是否可点 / 可用性筛选
  // / 详情里的「当前可用」四处都读它）。`keys` 只是**有计数时才存在**的补充说明 ——
  // 登录态由 `modelsToView()` 从 `available_keys` 填，游客兜底表（`data.js > MARKET`）
  // 按 rant 2026-08-19T15:54:06「虚构数据已移除」**不携带计数**。
  // 因此没有计数时**不能**读成「无 key」：那会让同一行的绿点 + 可点的「使用」按钮
  // 与这一格文案互相打脸（C2128 实测：游客市场 7/7 行都渲染成「无 key」，其中 6 行按钮可点）。
  function availPill(m) {
    const n = m.keys || 0;
    if (n >= 2) return '<span class="pill pill-ok">' + esc(T("mk.avail.multi", { n: n })) + "</span>";
    if (n === 1) return '<span class="pill pill-warn">' + esc(T("mk.avail.tight")) + "</span>";
    if (m.avail) return '<span class="pill pill-ok">' + esc(T("mk.avail.on")) + "</span>";
    return '<span class="pill pill-muted">' + esc(T("mk.avail.none")) + "</span>";
  }

  // P2-B：拉取市场真实模型（登录时）；失败 → 空态 + 重试（不 mock，rant 15:54:06）
  async function loadMarketplace() {
    if (!loggedIn()) return;
    if (!Live.models) {
      try {
        await liveLoad("models", "/api/models");
      } catch (e) { Live.models = null; /* 登录态降级空态 */ }
    }
    renderMarketplace();
    renderPrefModels(); // 设置页「偏好 → 默认模型」下拉同源真实模型表
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
      setLiveError($("#share-body"), loadErrorRow(8, T("share.loadFail"), T("err.loadFail")), () => loadSharing());
      return;
    }
    const on = list.filter((s) => s.status === "on");
    const totalEarned = list.reduce((a, s) => a + s.earned, 0);
    const totalUsed = list.reduce((a, s) => a + s.used, 0);
    // 本月新增（原型 4 张卡的第 4 张）：按上架时间（UTC）落在本月计数。
    // 月键与后端 SQL 同源取 created_at；拿不到 created_at 的行**不计入**（宁可少报，不误报）。
    const nowMonth = utcMonth(new Date().toISOString());
    const thisMonth = list.filter((s) => s.month && s.month === nowMonth);

    $("#share-stats").innerHTML = [
      stat(T("share.stats.listings"), T("cnt.keys", { n: on.length }), T("cnt.hist", { n: list.length })),
      stat(T("share.stats.earnings"), "+" + D.fmt(totalEarned) + " " + T("common.points"), T("share.stats.earnings.sub")),
      stat(T("share.stats.used"), D.fmt(totalUsed) + " " + T("common.points"), T("cnt.quota", { n: D.fmt(list.reduce((a, s) => a + s.quota, 0)) })),
      // 副标题**不复刻原型**：原型写死模型名 `deepseek-v4-flash`，而这张卡聚合的可能是多个模型
      // （原型自己的表格就渲染多行）⇒ 多个时只报计数，单个时才带模型名（见下）
      stat(T("share.stats.newthis"), T("cnt.keys", { n: thisMonth.length }),
        thisMonth.length === 1 ? thisMonth[0].model
          : (thisMonth.length ? T("share.stats.newthis.sub") : T("share.stats.newthis.none"))),
    ].join("");

    // 表单下拉（厂商 → Plan → 模型 三级联动；Plan 中「API」= 按量计价的 key）
    const selP = $("#sf-provider");
    const selPlan = $("#sf-plan");
    const selM = $("#sf-model");
    // 当前清单：优先 /api/plans（后端 config [[plans]] 单一真源），未登录/拉取失败降级 data.js。
    // 每次读（不是捕获一份副本）：登录后首次渲染时 `Live.plans` 还没回来，随后会被真实清单替换，
    // 而监听器只在第一次渲染时登记一次 —— 捕获副本的写法会让监听器永远指着那份兜底表。
    const plansSrc = () => Live.plans || D.PLANS;
    const fillModels = () => {
      const plan = plansSrc().find((pl) => pl.id === selPlan.value);
      const p = plan ? plan.provider : selP.value;
      // 零 mock（rant 15:54:06）：模型下拉登录态用 /api/models（Live.models），游客/兜底 data.js
      const modelSrc = Live.models ? Live.models : D.MODELS;
      selM.innerHTML = '<option value="">' + T("share.select.model") + "</option>" + modelSrc.filter((m) => !p || m.provider === p)
        .map((m) => '<option value="' + m.model + '">' + m.model + "</option>").join("");
      showPriceHint(selM.value);
    };
    const fillPlans = () => {
      const p = selP.value;
      selPlan.innerHTML = '<option value="">' + T("share.select.plan") + "</option>" + plansSrc().filter((pl) => pl.provider === p)
        .map((pl) => '<option value="' + pl.id + '">' + esc(planLabel(pl)) + "</option>").join("");
      showPlanHint("");
      fillModels();
    };
    // 监听器只登记一次（重建下拉框不该重复登记，否则一次 change 会级联跑两遍）
    if (!selP.dataset.wired) {
      selP.addEventListener("change", fillPlans);
      selPlan.addEventListener("change", () => { showPlanHint(selPlan.value); fillModels(); });
      selM.addEventListener("change", () => showPriceHint(selM.value));
      selP.dataset.wired = "1";
    }
    // 重建的判据是**数据源**，不是「建过没有」（C2133）：登录后首次渲染时 /api/plans 还在路上，
    // 兜底表会先建一次；若按「建过就跳过」，真实清单回来后下拉框**永远**不重建 —— 那个一次性
    // 守卫等于让兜底表赢到底，`planLabel` 也就永远没机会生效（en 界面上就是兜底表里的中文名）。
    const src = Live.plans ? "live" : "fallback";
    if (selP.dataset.plansSrc !== src) {
      selP.innerHTML = '<option value="">' + T("share.select.provider") + "</option>" +
        [...new Set(plansSrc().map((pl) => pl.provider))]
          .map((p) => '<option value="' + p + '">' + esc(provLabel(p)) + "</option>").join("");
      selP.dataset.plansSrc = src;
      fillPlans();
    }

    $("#share-body").innerHTML = list.length ? list.map((s, i) => {
      // 已用 / 额度 进度条（rant 2026-09-11T16:23:43 第 5 节：进度条 + 数字）
      const pct = s.quota > 0 ? Math.min(100, Math.round((s.used / s.quota) * 100)) : 0;
      return "<tr><td data-label='" + T('share.col.provider') + "'><strong>" + esc(provLabel(s.provider)) + " · " + esc(s.plan || "API") +
      "</strong><div class='muted' style='font-size:12px'>" + esc(s.model) + "</div></td>" +
      "<td data-label='" + T('share.col.key') + "' class='mono'>" + esc(maskKey(s.key)) + "</td>" +
      "<td data-label='" + T('share.col.used') + "' class='num'>" + D.fmt(s.used) + " / " + D.fmt(s.quota) +
      '<div class="bar-track" style="margin-top:5px"><div class="bar-fill' + (pct >= 100 ? " alt" : "") + '" style="width:' + pct + '%"></div></div></td>' +
      '<td class="num" data-label="' + T("share.col.price") + '">' + D.fmt(s.price) + " " + T("share.priceUnit") + "</td>" +
      '<td class="num" data-label="' + T("share.col.earn") + '">+' + D.fmt(s.earned) + " " + T("common.points") + "</td>" +
      "<td data-label='" + T('share.col.avail') + "'>" + esc(fmtAvailable(s)) + "</td>" +
      "<td data-label='" + T('share.col.status') + "'>" + badge(s.status, SHARE_STATUS) + "</td>" +
      "<td data-label='" + T('share.col.action') + "'><button class='btn btn-ghost btn-sm' data-share-toggle='" + i + "'>" +
      (s.status === "on" ? T("share.toggle.pause") : s.status === "paused" ? T("share.toggle.resume") : T("share.toggle.relist")) + "</button> " +
      "<button class='btn btn-danger btn-sm' data-share-delete='" + i + "'>" + T("common.delete") + "</button></td></tr>";
    }).join("") : emptyRow(8, T("share.empty"), T("share.empty.sub"),
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

  // 点数方向由 `type` 决定，不由 `pts` 的符号决定（C2047）：
  // 账本按 type 编码方向，所有生产 writer 都把 pts 存成非负数 —— `billing::settle` 的
  // consume 行存正数、随后的 earn 行也存正数（src/billing.rs），符号只表示「数值」，不表示「收支」。
  // 服务端每个聚合都是按 type 判方向（src/routes/wallet.rs 的 TX_INCOME_TYPES / TX_EXPENSE_TYPES、src/routes/ops.rs:94/103），
  // 前端必须用同一约定，否则「消费」在汇总卡里是支出（`expense_pts`）、在明细行里却被渲染成绿色的 `+3.7`。
  const PTS_INCOME_TYPES = { earn: true, topup: true, gift: true };
  const signedPts = (type, pts) => (PTS_INCOME_TYPES[type] === true ? pts : -pts);

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
      // C2047：服务端 net 已是「收入 − 消费」的有符号值，不能再取反
      net = Live.dashboard.net || 0;
      // C2047：逐类型求和后先转成带符号的值再交给 monthChangeItem（它按符号渲染，本来就正确）
      rowsHtml = MONTH_TYPE_LABELS
        .filter(([k]) => sums[k])
        .map(([k, label]) => monthChangeItem(label(), signedPts(k, sums[k]), false)).join("");
      const series = Live.dashboard.series || [];
      sparkData = series.map((s) => s.pts || 0);
      sparkLabels = series.map((s) => localMD(String(s.date || "")));
    } else if (!loggedIn()) {
      // 游客演示：data.js 内嵌交易聚合（mock 仅游客，rant 15:54:06）
      const txs = D.TRANSACTIONS || [];
      const sums = {};
      txs.forEach((t) => { sums[t.type] = (sums[t.type] || 0) + t.pts; });
      // C2047：同一约定 —— 逐类型转符号后求和（未来若 mock 里出现未知类型，按服务端的 `ELSE -pts` 口径视为支出）
      net = Object.keys(sums).reduce((a, k) => a + signedPts(k, sums[k]), 0);
      rowsHtml = MONTH_TYPE_LABELS
        .filter(([k]) => sums[k])
        .map(([k, label]) => monthChangeItem(label(), signedPts(k, sums[k]), false)).join("");
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
    // 永久点数（原型 wallet-hero hint）：/api/wallet 的 balance 即永久余额（available = 永久 + 当日赠送）
    const fv = $("#wallet-forever");
    if (fv) {
      const w = Live.wallet;
      fv.textContent = D.fmt(w && typeof w.balance === "number" ? w.balance : D.USER.balance);
    }
    // 本月点数变化（近 1 月按类型汇总收支，与仪表盘一致）
    renderMonthChanges();
  }

  /* --- 充值模拟（US-4：钱包页行内卡片 → 输入点数 → 余额增加 + topup 交易，永久有效点数） --- */

  function openTopup() {
    $("#topup-custom").value = "";
    $$("#topup-card .topup-presets .chip").forEach((b) => b.classList.remove("on"));
    $("#topup-card").hidden = false;
    $("#raise-card").hidden = true; // 互斥：开充值收起加额
    clearFieldError($("#topup-custom"));
    $("#topup-custom").focus();
  }

  function closeTopup() {
    $("#topup-card").hidden = true;
  }

  function confirmTopup() {
    const preset = document.querySelector("#topup-card .topup-presets .chip.on");
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
      setLiveError(el, loadErrorHtml(T("admin.raise.loadFail"), T("err.loadFail")), () => loadAdmin());
      return;
    }
    // 表头在渲染期取 i18n（本表由 JS 在 boot 之后注入，无 data-i18n 钩子可走）：
    // 既保证首屏语言正确，也让 atp:langchange → renderView 能实时换语言
    // C2126：已处理行里显示的是「处理时间」，必须交给时间 helper（`timeCell` → 本地化、
    // 秒精度、悬停给相对时间），不能自己切服务端串 —— `created_at` 是
    // `dao::utc_iso` 的 `YYYY-MM-DDTHH:MM:SSZ`，`slice(5, 16)` 会同时泄露 ISO 的 `T`
    // 分隔符（屏幕上真的出现 `09-13T16:30`）并按 UTC 显示小时。
    el.innerHTML = (list.length ? '<div class="table-wrap compact"><table class="table"><thead><tr>' +
      '<th>' + esc(T("admin.raise.col.member")) + "</th>" +
      '<th class="num">' + esc(T("admin.raise.col.amount")) + "</th>" +
      "<th>" + esc(T("admin.raise.col.reason")) + "</th>" +
      "<th>" + esc(T("admin.raise.col.status")) + "</th>" +
      "<th></th></tr></thead><tbody>" +
      list.map((r, i) =>
        "<tr><td data-label='" + T('admin.raise.col.member') + "'><strong>" + esc(r.name || r.user) + "</strong><div class='muted' style='font-size:12px'>" + esc(r.email) + "</div></td>" +
        '<td class="num" data-label="' + T("admin.raise.col.amount") + '">+' + D.fmt(r.amount) + " " + T("common.points") + "</td>" +
        "<td data-label='" + T('admin.raise.col.reason') + "'>" + esc(r.reason) + "</td>" +
        "<td data-label='" + T('admin.raise.col.status') + "'>" + badge(r.status, RAISE_STATUS) + "</td>" +
        "<td data-label='" + T('admin.raise.col.action') + "'>" + (r.status === "pending"
          ? "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-raise-approve='" + i + "'>" + T("admin.raise.approve") + "</button> " +
            "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-raise-reject='" + i + "'>" + T("admin.raise.reject") + "</button>"
          : '<span class="muted" style="font-size:12px">' + timeCell(r.created_at, true) + "</span>") + "</td></tr>"
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
    withdraw: () => T("tx.type.withdraw"), gift: () => T("tx.type.gift"), expire: () => T("tx.type.expire"),
  };
  const txType = (k) => (TX_TYPE[k] ? TX_TYPE[k]() : k);
  const txStatus = (s) => s === "成功" ? T("tx.status.success") : s === "处理中" ? T("tx.status.pending") : s === "入账" ? T("tx.status.credited") : s;

  // 交易表的列筛选**只有服务端一个实现**（rant 2026-08-25T10:33:26）：每个带 `filter` 的列都声明
  // `serverFilter: true`，含义是「这列的筛选值随请求发给服务端（`txFilterParams`）」⇒ 客户端
  // 不得再用 `filterRows` 本地筛一遍（同一条规则的第二份实现，归一化与语义都不同，见 `filterRows`）。
  // 新增带 `filter` 的列时**必须**一并声明它 —— 少了它，本地就会多筛一层。
  const TX_COLUMNS = [
    // rant 2026-08-23T16:01:07：列表内移除 time 列筛选（外部 tx-range 已有时间段筛选，两套并存冗余）
    { key: "time", title: () => T("tx.col.time"), sort: "string", render: (t) => timeCell(t.time, true) },
    { key: "type", title: () => T("tx.col.type"), sort: "string", filter: "select", serverFilter: true,
      // C2031：选项「值」与「文案」分离 —— value 恒为数据库值（语言无关），label 才随语言变。
      // 这样筛选状态存的是 DB 值，切换语言后 filterVal 仍能匹配（此前状态存本地化文案 ⇒ 切语言即失配，表格变空）
      options: () => ["consume", "earn", "topup", "withdraw", "gift", "expire"].map((k) => ({ value: k, label: txType(k) })),
      filterVal: (t) => t.type,
      render: (t) => t.type === "earn" ? '<span class="pill pill-ok">' + T("tx.type.earn") + "</span>" : t.type === "consume" ? '<span class="pill pill-accent">' + T("tx.type.consume") + "</span>" : t.type === "gift" ? '<span class="pill pill-ok">' + T("tx.type.gift") + "</span>" : '<span class="pill pill-muted">' + esc(txType(t.type)) + "</span>" },
    // rant 2026-08-22T17:21:39 需求 2：新增「用户」列（transactions.user_id JOIN users 取用户名）
    { key: "user", title: () => T("tx.col.user"), sort: "string", filter: "text", serverFilter: true },
    { key: "model", title: () => T("tx.col.model"), sort: "string", filter: "text", serverFilter: true },
    // rant 2026-08-22T17:21:39 需求 2：Key 列改为显示分发 key 的 name（api_keys.name），而非上游 keys 的 provider/plan
    { key: "key", title: () => T("tx.col.apiKeyName"), sort: "string", filter: "text", serverFilter: true },
    // C2102：四列 Token 的排序键不能在行对象上找 —— 行里存的是 K/M **显示串**（`tokens`/`inputTokens`…）
    // 与精确值两套字段（`tokensRaw`/`inputRaw`…），列 key 与行字段名不同源 ⇒ 默认 `row[key]` 取到
    // undefined（input/cached/output）或显示串（tokens），`Number()` 得 NaN ⇒ 比较器恒「相等」，
    // 点列头永不动行。与 C2054（pts 的 `sortVal`）同一条规则：**排序必须与单元格里的数字同口径**。
    { key: "input", title: () => T("tx.col.input"), sort: "number", align: "num",
      render: (t) => '<span' + t.tokenBrk(T("tx.col.input"), t.inputRaw) + '>' + t.inputTokens + "</span>",
      sortVal: (t) => t.inputRaw },
    { key: "cached", title: () => T("tx.col.cached"), sort: "number", align: "num",
      render: (t) => '<span' + t.tokenBrk(T("tx.col.cached"), t.cachedRaw) + '>' + t.cachedTokens + "</span>",
      sortVal: (t) => t.cachedRaw },
    { key: "output", title: () => T("tx.col.output"), sort: "number", align: "num",
      render: (t) => '<span' + t.tokenBrk(T("tx.col.output"), t.outputRaw) + '>' + t.outputTokens + "</span>",
      sortVal: (t) => t.outputRaw },
    { key: "tokens", title: () => T("tx.col.tokens"), sort: "number", align: "num",
      render: (t) => '<span' + t.tokenBrk(T("tx.col.tokens"), t.tokensRaw) + '>' + t.tokens + "</span>",
      sortVal: (t) => t.tokensRaw },
    { key: "pts", title: () => T("tx.col.pts"), sort: "number", filter: "number-range", align: "num", serverFilter: true,
      // C2047：方向取自 type（signedPts），不能按 pts 的符号判 —— 所有 writer 都存正数 ⇒ 消费会显示成绿色的 +N
      render: (t) => {
        const v = signedPts(t.type, t.pts);
        return '<span style="color:' + (v > 0 ? "var(--ok)" : v < 0 ? "var(--danger)" : "var(--text)") + ';font-weight:600">' + (v > 0 ? "+" : "") + D.fmt(v) + "</span>";
      },
      // C2054：筛选/排序必须与**渲染值**同口径 —— 用户是按单元格里看到的数字筛选与排序的。
      // 少了这两行，`filterRows`/排序会退回 `row.pts`（库内正数）⇒ 按 -3.7 筛选得 0 行、
      // 按 3 到 4 反而命中那条「-3.7」的行。
      filterVal: (t) => signedPts(t.type, t.pts),
      sortVal: (t) => signedPts(t.type, t.pts) },
    { key: "status", title: () => T("tx.col.status"), sort: "string", filter: "select", serverFilter: true,
      // C2031：同 type —— value 是库内值（成功/入账/处理中，语言无关），label 随语言变
      options: () => ["成功", "入账", "处理中"].map((s) => ({ value: s, label: txStatus(s) })),
      filterVal: (t) => t.status,
      render: (t) => t.status === "处理中" ? '<span class="pill pill-warn">' + esc(txStatus(t.status)) + "</span>" : esc(txStatus(t.status)) },
  ];

  // 交易汇总卡（rant 2026-09-11T16:23:43 第 7 节：改用原型 .stat-grid + .stat-card；
  // 数据口径不变 —— 后端 summary 全量 SQL 聚合（含列筛选），不再对当前页本地加总；
  // 口径 = income 白名单（earn/topup/gift）为正、consume 为负；附带 Token 总/输入/缓存/输出）
  function renderTxSummary(list) {
    const el = $("#tx-summary");
    if (!el) return;
    const s = (Live.transactions && Live.transactions.summary) ? Live.transactions.summary : null;
    // rant 2026-08-25T10:33:26：后端 summary 已随列筛选全量 SQL 聚合（income 白名单 earn/topup/gift
    // 为正、consume 为负；token 口径一致）——登录态一律用后端 summary，本地加总仅作无 summary 的兜底。
    let income = 0, expense = 0, tokens = 0, inputT = 0, cachedT = 0, outputT = 0, count = 0;
    if (s) {
      income = s.income_pts || 0;
      expense = s.expense_pts || 0;
      tokens = s.tokens || 0;
      inputT = s.input_tokens || 0;
      cachedT = s.cached_tokens || 0;
      outputT = s.output_tokens || 0;
    } else {
      // 兜底（无 summary 的旧数据/游客 mock）：按 type 而非 pts 符号（rant 00:04:21 Bug A）
      list.forEach((t) => {
        if (t.type === "consume") expense += Math.abs(t.pts);
        else income += Math.abs(t.pts);
        if (typeof t.tokensRaw === "number") tokens += t.tokensRaw;
        if (typeof t.inputRaw === "number") inputT += t.inputRaw;
        if (typeof t.cachedRaw === "number") cachedT += t.cachedRaw;
        if (typeof t.outputRaw === "number") outputT += t.outputRaw;
      });
    }
    // 记录数 = 后端 total（真分页下即筛选项下的全量条数；无 total 时退化为当前可见行数）
    count = (Live.transactions && typeof Live.transactions.total === "number")
      ? Live.transactions.total
      : list.length;
    const net = income - expense;
    const signed = (n) => (n > 0 ? "+" : n < 0 ? "-" : "") + D.fmt(Math.abs(n));
    const colour = (n) => (n > 0 ? "var(--ok)" : n < 0 ? "var(--danger-text)" : "inherit");
    const fmtM = (n) => (n >= 1e6 ? (n / 1e6).toFixed(2) + "M" : (n >= 1000 ? Math.round(n / 1000) + "K" : String(Math.round(n))));
    // 复用 PR4 的 stat()（.stat-card）与原型卡片顺序：消费 / 收益 / 点数变化 / Token 合计 / 记录数；
    // Token 卡的 sub 承载输入·缓存·输出三档明细（原型「三档 token 之和」口径）
    el.innerHTML =
      stat(T("tx.summary.expense"), D.fmt(Math.abs(expense)), T("tx.summary.expense.sub")) +
      stat(T("tx.summary.income"), '<span style="color:' + colour(income) + '">' + signed(income) + "</span>", T("tx.summary.income.sub")) +
      stat(T("tx.summary.net"), '<span style="color:' + colour(net) + '">' + signed(net) + "</span>", T("tx.summary.net.sub")) +
      stat(T("tx.summary.tokens"), fmtM(tokens),
        T("tx.summary.brk", { i: fmtM(inputT), c: fmtM(cachedT), o: fmtM(outputT) })) +
      stat(T("tx.summary.count"), D.fmt(count), T("tx.summary.count.sub"));
  }

  // 每日消费/收益趋势（rant 2026-09-11T16:23:43 第 7 节 + PR4 的 renderDashTrend 先例）：
  // 改用原型 .trend 双色柱状图（消费 = accent、收益 = ok），柱高按当日 max 归一、最小 2%；
  // 数据源仍是 /api/transactions/trend（与列表同 type/时间段/列筛选口径）。
  // GROUP BY 只返回有交易的桶（无交易的日子缺行）→ 按请求窗口补 0，保持 x 轴左→右时间递增、
  // 柱距恒定（原型 x 轴递增 bug 的根因即「缺行导致柱子左移」，此处一并规避）。
  const TX_TREND_MAX_COLS = 40; // 桶数上限（hour 桶 24h 窗口 + 余量），超出则抽稀标签
  function txTrendDays(buckets, bucket) {
    const byKey = new Map();
    const keyOf = (d) => {
      const p = (n) => String(n).padStart(2, "0");
      const md = d.getUTCFullYear() + "-" + p(d.getUTCMonth() + 1) + "-" + p(d.getUTCDate());
      return bucket === "hour" ? md + "-" + p(d.getUTCHours()) : md;
    };
    buckets.forEach((b) => {
      const d = new Date(b.t);
      if (!isNaN(d.getTime())) byKey.set(keyOf(d), b);
    });
    const now = new Date();
    // 窗口右端：custom/end 已指定则用 end，否则用「现在」
    // （UTC 对齐到桶，与后端 strftime 口径一致：hour→小时、day→当日、week→周一）
    let end;
    if (txRange === "custom" && txCustomEnd) {
      end = new Date(txCustomEnd);
      if (isNaN(end.getTime())) end = now;
    } else {
      end = now;
    }
    const p = (n) => String(n).padStart(2, "0");
    // 桶起点 UTC 对齐：hour → 所在小时；day → 所在 UTC 日的 00:00；
    // week → 所在自然周的**周一** 00:00（`(getUTCDay()+6)%7` 即「距本周一的天数」，周一→0）。
    // week 必须对齐到周一，才能与后端 strftime 的 `weekday 0, -6 days` 同口径 ——
    // 否则补出的轴键是「今天减 7k 天」（星期几跟随今天），6/7 的星期几对不上后端桶键，
    // 整周柱子被补成 0（见 C2094）。
    const utcFloor = (d) => bucket === "hour"
      ? Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), d.getUTCHours())
      : bucket === "week"
      ? Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() - ((d.getUTCDay() + 6) % 7))
      : Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
    const step = bucket === "hour" ? 36e5 : bucket === "week" ? 7 * 864e5 : 864e5;
    // 窗口长度：跟随实际数据跨度（先算最早桶到右端的桶数），避免 24h/自定义短窗口补出几百根空柱
    const earliest = buckets.length
      ? buckets.reduce((min, b) => { const t = new Date(b.t).getTime(); return isNaN(t) ? min : Math.min(min, t); }, Infinity)
      : NaN;
    let cols = isFinite(earliest) ? Math.round((utcFloor(end) - utcFloor(new Date(earliest))) / step) + 1 : 1;
    cols = Math.max(1, Math.min(cols, TX_TREND_MAX_COLS));
    const start = utcFloor(end) - (cols - 1) * step;
    const days = [];
    for (let i = 0; i < cols; i++) {
      const d = new Date(start + i * step);
      const key = keyOf(d);
      const b = byKey.get(key);
      days.push(b ? Object.assign({ t: d.toISOString() }, b) : { t: d.toISOString(), income: 0, expense: 0, tokens: 0, count: 0 });
    }
    return days;
  }

  function renderTxTrend() {
    const el = $("#tx-trend");
    if (!el) return;
    const tr = (Live.transactions && Live.transactions.trend) ? Live.transactions.trend : null;
    const raw = (tr && Array.isArray(tr.buckets)) ? tr.buckets : [];
    if (!raw.length) {
      // 空态：未加载 / 加载失败 / 窗口内无交易，三者文案区分（零 mock 语义）
      el.innerHTML = '<div class="empty compact">' +
        esc(tr === null ? T("dash.trend.fail") : T("tx.trend.empty")) + "</div>";
      return;
    }
    const bucket = tr.bucket || "day";
    const days = txTrendDays(raw, bucket);
    const max = Math.max(1, ...days.map((b) => Math.max(b.expense || 0, b.income || 0)));
    el.innerHTML = days.map((b) => {
      const c = b.expense || 0, e = b.income || 0;
      const lbl = bucketLabel(b.t, bucket);
      const h = (v) => Math.max(2, (v / max) * 100).toFixed(1);
      const tip = lbl + " " + T("tx.trend.metric.expense") + " " + D.fmt(c) + " / " + T("tx.trend.metric.income") + " " + D.fmt(e);
      return '<div class="trend-col" title="' + esc(tip) + '"><div class="trend-pair">' +
        '<div class="trend-bar consume" style="height:' + h(c) + '%"></div>' +
        '<div class="trend-bar earn" style="height:' + h(e) + '%"></div>' +
        '</div><span class="trend-x">' + esc(lbl) + "</span></div>";
    }).join("");
  }

  // 趋势桶标签：hour → "MM-DD HH:00"；day/week → "MM-DD"（bucket 起点均为 UTC，转本地显示）
  function bucketLabel(iso, bucket) {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    const p = (n) => String(n).padStart(2, "0");
    const md = p(d.getMonth() + 1) + "-" + p(d.getDate());
    return bucket === "hour" ? md + " " + p(d.getHours()) + ":00" : md;
  }

  // 趋势聚合粒度：跟随外部时间段筛选（24h→小时；≤3.5 天→小时；≤60 天→天；其余→周）
  function txTrendBucket() {
    if (txRange === "24h") return "hour";
    if (txRange === "7d") return "day";
    if (txRange === "30d") return "day";
    if (txRange === "custom") {
      const s = txCustomStart ? new Date(txCustomStart) : null;
      const e = txCustomEnd ? new Date(txCustomEnd) : null;
      const now = new Date();
      const from = (s && !isNaN(s.getTime())) ? s : ((e && !isNaN(e.getTime())) ? new Date(e.getTime() - 30 * 86400000) : now);
      const to = (e && !isNaN(e.getTime())) ? e : now;
      const days = (to.getTime() - from.getTime()) / 86400000;
      if (days <= 3.5) return "hour";
      if (days <= 60) return "day";
      return "week";
    }
    return "week"; // all：跨度过大按周聚合
  }

  function renderTransactions() {
    // tab 高亮 = **生效的类型筛选**（txTypeFilter()），不是另存的一份 tab 状态 —— 两处必须同源，
    // 否则列筛选一出值，高亮就与列表说的不是同一件事。生效值不属于 all/consume/earn 时
    // （topup/withdraw/gift/expire），没有任何 tab 可以自称生效 ⇒ 都不高亮。
    const effTab = txTypeFilter() || "all";
    $$("#tx-tabs .tab").forEach((b) => b.classList.toggle("active", b.dataset.txTab === effTab));
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不 fallback D.TRANSACTIONS；
    // 加载失败 → 空态 + 重试；游客不可达（导航拦截）
    if (loggedIn() && !Live.transactions) {
      renderTxSummary([]);
      renderTxTrend();
      const c = $("#tx-count");
      if (c) c.textContent = "";
      setLiveError($("#tx-table"), loadErrorHtml(T("tx.loadFail"), T("err.loadFail")), () => loadTransactions());
      return;
    }
    // 真后端分页 + 列筛选（rant 2026-08-24T10:51:57 + 2026-08-25T10:33:26）：页码/每页行数/筛选条件
    // 任一与已加载不一致 → 重新向后端拉取对应页（筛选变化同样触发，不再只过滤本地当前页）
    if (Live.transactions && (txTable.loadedPage !== txTable.page || txTable.loadedPageSize !== txTable.pageSize || txTable.loadedFilterSig !== txFilterSig())) {
      loadTransactions();
      return;
    }
    let list = Live.transactions ? txsToView(Live.transactions.items || []) : [];
    // 交易汇总卡：与 tab + 列筛选联动，与表格可见行一致（rant 20:39:30 B；记录数走后端 total）
    renderTxSummary(filterRows(list, TX_COLUMNS, txTable.filters));
    // 记录数（原型 #tx-count）：后端 total（真分页下即筛选后的全量条数）
    const cntEl = $("#tx-count");
    if (cntEl) {
      const n = (Live.transactions && typeof Live.transactions.total === "number")
        ? Live.transactions.total
        : filterRows(list, TX_COLUMNS, txTable.filters).length;
      cntEl.textContent = (Live.transactions || !loggedIn()) ? T("tx.pager.count", { n: n }) : "";
    }
    // 趋势图：跟随 tab + 外部时间段（rant 2026-08-23T16:01:07 需求 2）
    renderTxTrend();
    buildDataTable({
      container: $("#tx-table"),
      columns: TX_COLUMNS,
      rows: list,
      state: txTable,
      onState: renderTransactions,
      // 真后端分页（rant 2026-08-24T10:51:57）：总数显示后端 total；页码点击 → onState → renderTransactions 页码不一致自动重拉
      serverPaging: Live.transactions ? { total: Live.transactions.total || 0 } : null,
    });
  }

  // 交易时间段 → start/end 查询参数（UTC ISO，后端 datetime() 解析；默认最近 24 小时）
  function txRangeParams() {
    const now = new Date();
    const MS = (h) => h * 3600 * 1000;
    let start = null, end = null;
    if (txRange === "24h") start = new Date(now - MS(24));
    else if (txRange === "7d") start = new Date(now - MS(24 * 7));
    else if (txRange === "30d") start = new Date(now - MS(24 * 30));
    else if (txRange === "custom") {
      if (txCustomStart) start = new Date(txCustomStart);
      if (txCustomEnd) end = new Date(txCustomEnd);
    }
    const p = [];
    if (start && !isNaN(start.getTime())) p.push("start=" + encodeURIComponent(start.toISOString()));
    if (end && !isNaN(end.getTime())) p.push("end=" + encodeURIComponent(end.toISOString()));
    return p.join("&");
  }

  // 列筛选 → 后端全量过滤查询参数（rant 2026-08-25T10:33:26：列筛选不再只过滤当前加载页）。
  // 列 key → 后端参数：user→user_name、key→key_name、pts 区间→pts_min/pts_max、status 精确。
  // select 列（type/status）的筛选值**恒为库内值**（C2031：选项 value 与 i18n 文案分离），直接发送。
  // ⚠️ 这里曾有 TX_TYPE_INV / TX_STATUS_INV 两张「显示文案 → 库内值」反查表；它们以**运行期语言输出**为键，
  //    只能表达「按下拉当前显示的文案反查」，而筛选状态一旦存文案，切换语言就会与重新本地化的比较值失配
  //    （表格变空）。改成「状态存库内值」后反查表无事可做，故整体删除 —— 消除该类，而非修个例。
  function txFilterParams() {
    const f = txTable.filters || {};
    const p = [];
    const add = (k, v) => { const s = String(v == null ? "" : v).trim(); if (s !== "") p.push(k + "=" + encodeURIComponent(s)); };
    if (f.user) add("user_name", f.user);
    if (f.model) add("model", f.model);
    if (f.key) add("key_name", f.key);
    if (f.pts) {
      const parts = String(f.pts).split(":");
      if (parts[0] !== "") add("pts_min", parts[0]);
      if (parts[1] !== "" && parts[1] != null) add("pts_max", parts[1]);
    }
    if (f.status) add("status", f.status);
    return p.join("&");
  }
  // 交易类型筛选：**一份状态，两套控件**。顶部 tab（全部/消费/收益）与「类型」列筛选 select
  // （6 个库内值）是同一个筛选器的两种控件，状态只存在 `txTable.filters.type`（空串 = 不限，
  // 取值恒为库内值 ⇒ 语言无关，见 C2031）。请求参数、tab 高亮、列筛选控件三者都必须是**它**的
  // 投影；此前顶部 tab 另存一份 `txTab`，而请求按 `filters.type || txTab` 取值 ⇒ 列筛选一出值，
  // tab 的写入就被永久盖住（点 tab 只是挪高亮、列表不变），高亮也仍按 `txTab` 画
  // ⇒ 指示器与列表说的不是同一件事。删掉第二份状态，该类不再存在。
  function txTypeFilter() {
    return (txTable.filters && txTable.filters.type) || "";
  }
  // 写类型筛选，并同步**已渲染**的列筛选 select：#148 之后表头不重建（保住筛选框焦点），
  // 控件值不会自己跟上状态；表头若尚未渲染/已被清空则无需同步，下次重建会读 state 得到正确值。
  function setTxTypeFilter(v) {
    txTable.filters = txTable.filters || {};
    txTable.filters.type = v || "";
    const sel = document.querySelector('#tx-table select[data-filter-key="type"]');
    if (sel) sel.value = txTable.filters.type;
  }
  // 列筛选签名：筛选条件变化 → renderTransactions 触发重拉（rant 2026-08-25T10:33:26）
  function txFilterSig() {
    const f = txTable.filters || {};
    return Object.keys(f).sort().map((k) => k + "=" + String(f[k] == null ? "" : f[k])).join("&");
  }

  // P2-B：按 tab 拉取交易（真后端分页 rant 2026-08-24T10:51:57：页码/每页行数随请求发出；
  // loadedPage/loadedPageSize 记录已加载页，renderTransactions 发现页码不一致时自动重拉）
  async function loadTransactions() {
    if (!loggedIn()) return;
    // 类型：单一状态（txTypeFilter()），tab 与列筛选都读写它；后端化后同走 type 参数。
    // C2031：筛选状态即库内值（consume/earn/…），直接发送，无需反查。
    const type = txTypeFilter();
    const range = txRangeParams();
    const cols = txFilterParams(); // rant 2026-08-25T10:33:26：列筛选随请求发出，后端全量过滤
    const page = Math.max(1, txTable.page || 1);
    const pageSize = Math.min(100, Math.max(1, txTable.pageSize || 10));
    const q = "/api/transactions?type=" + type + "&page=" + page + "&page_size=" + pageSize + (range ? "&" + range : "") + (cols ? "&" + cols : "");
    // 趋势图数据（rant 2026-08-23T16:01:07 需求 2）：同列筛选口径
    const bucket = txTrendBucket();
    const tq = "/api/transactions/trend?type=" + type + "&bucket=" + bucket + (range ? "&" + range : "") + (cols ? "&" + cols : "");
    try {
      // rant 2026-08-25T12:02:13：列表与趋势并行拉取（页面加载不再串行多等一个 ~0.9s 请求）；
      // 失败互不阻塞：列表失败降级空态，趋势失败仅 trend=null
      const [, trend] = await Promise.all([
        liveLoad("transactions", q).catch(() => { Live.transactions = null; return null; }),
        api.get(tq).catch(() => null),
      ]);
      if (Live.transactions) Live.transactions.trend = trend;
      txTable.loadedPage = page;
      txTable.loadedPageSize = pageSize;
      txTable.loadedFilterSig = txFilterSig(); // 记录已加载的筛选条件，变化时 renderTransactions 重拉
    } catch (e) { Live.transactions = null; /* 登录态降级空态 */ }
    renderTransactions();
    // 翻页后滚动到列表顶部（rant 2026-08-24T10:51:57 需求 4）
    if (page > 1) {
      const tbl = $("#tx-table");
      if (tbl && tbl.scrollIntoView) tbl.scrollIntoView({ block: "start" });
    }
  }

  // 交易记录导出 CSV（rant 20:46:57 E：Blob + a[download]，UTF-8 BOM，文件名 aitokenpool-transactions-YYYYMMDD.csv；导出当前筛选可见行）
  function exportTxCsv() {
    // 零 mock（rant 15:54:06）：登录态用后端数据，失败直接提示
    if (loggedIn() && !Live.transactions) { toast(T("tx.export.none"), "info"); return; }
    let list = Live.transactions ? txsToView(Live.transactions.items || []) : [];
    // 与表格可见行一致：列筛选由**服务端**施加（列上声明了 `serverFilter`，见 TX_COLUMNS），
    // 故这里不需要也不得本地再筛一遍 —— 服务端返回的行就是表格显示的行（C2114）。
    list = filterRows(list, TX_COLUMNS, txTable.filters);
    if (!list.length) { toast(T("tx.export.none"), "info"); return; }
    const cell = (v) => { const s = String(v == null ? "" : v); return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s; };
    const headers = [T("tx.col.time"), T("tx.col.type"), T("tx.col.user"), T("tx.col.model"), T("tx.col.apiKeyName"), T("tx.col.input"), T("tx.col.cached"), T("tx.col.output"), T("tx.col.tokens"), T("tx.col.pts"), T("tx.col.status")];
    // C2054：导出的是「当前筛选可见行」，故各列与表格单元格**同口径** —— 点数写有符号值，
    // 否则屏幕上写着 -3.7、导出的文件里却是 3.7。
    // C2111：时间列同理。单元格渲染的是**本地**精确时间（`fmtPrecise`，时间列口径见 #139
    // rant 2026-08-24T12:38:44），而导出一直直接写 `t.time`（库内 UTC 串，`txsToView` 未转换）
    // ⇒ 同一行在表里是 23:04、在文件里却是 15:04（东八区；西半球反向）。两处必须同源：
    // 导出直接用渲染该单元格的同一个 helper，而不是再抄一份「时间转字符串」的口径。
    const lines = list.map((t) => [fmtPrecise(t.time), txType(t.type), t.user, t.model, t.key, t.inputTokens, t.cachedTokens, t.outputTokens, t.tokens, signedPts(t.type, t.pts), txStatus(t.status)].map(cell).join(","));
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

  /* --- 数据表格键盘导航（rant 20:46:57 F：↑/↓ 行高亮 .row-active，Enter 触发主操作，Esc 清除） ---
     容器由 DOM 派生（事件目标最近的 <tbody>），不维护「哪些表格可导航」的名册：
     数据表一律可导航，新增表格无需登记，重绘也无需重新绑定。 */
  let kbd = { c: null, i: -1 }; // 当前激活的表格容器 + 高亮行下标

  function kbdRows(c) {
    if (!c || c.isConnected === false) return []; // 表格被整体重建 ⇒ 记住的容器已失效，视为未激活
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
  // 数据表容器 = 事件目标最近的 <tbody>（判断依据是元素本身，不是它的 id 是否被某个名册登记过）
  function kbdTbodyOf(t) {
    return (t && t.closest) ? t.closest("tbody") : null;
  }
  function kbdContainerFrom(t) {
    // 键盘事件的目标通常是 body（无 tbody 可寻）⇒ 沿用上次激活的表格
    return kbdTbodyOf(t) || (kbd.c && kbd.c.isConnected !== false ? kbd.c : null);
  }

  /* --- 通用 MRT 风格数据表格渲染器 ---
     cfg: { container, columns, rows, state, onState }
     columns: [{ key, title, sort?: "string"|"number", filter?: "text"|"select"|"number-range", options?, render? }]
     state:  { sort: [{key,dir}], filters: {key:val}, page, pageSize }（原地更新，跨页保留） */

  // 按列筛选条件过滤行（buildDataTable 与交易汇总条共用，保证汇总与表格可见行一致）
  //
  // ⚠️ 只筛**客户端自己筛**的列。交易表的列筛选由**服务端**施加（rant 2026-08-25T10:33:26：列筛选
  // 从「本地过滤当前页」改为「后端全量过滤」），那些列声明 `serverFilter: true` ⇒ 这里跳过。
  // 理由：服务端已按**同一组**筛选返回了行，本地再筛一遍等于同一规则的**第二份实现**，而两边的
  // 归一化与匹配语义并不相同 —— 请求侧 `txFilterParams` 先 `trim()`、服务端用 SQL `LIKE`，本函数
  // 既不去空白也不认通配符 ⇒ 用户输入 `"deepseek "`（尾随空格）时服务端命中并返回该行，本地却把它
  // 删掉：表格空，而「共 N 条」与汇总卡仍按 N 显示（C2114）。消除副本，而不是让副本跟上。
  function filterRows(rows, columns, filters) {
    return rows.filter((row) => {
      for (const key of Object.keys(filters)) {
        const fv = filters[key];
        if (fv == null || fv === "") continue;
        const col = columns.find((c) => c.key === key);
        if (!col || !col.filter || col.serverFilter) continue;
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

  // 紧凑分页码（页数 > 9 时省略号收拢：1 … p-1 p p+1 … N；页数少则全量渲染）
  function pagerButtons(page, pages) {
    const out = [];
    if (pages <= 9) { for (let i = 1; i <= pages; i++) out.push(i); return out; }
    out.push(1);
    if (page > 4) out.push("…");
    for (let i = Math.max(2, page - 1); i <= Math.min(pages - 1, page + 1); i++) out.push(i);
    if (page < pages - 3) out.push("…");
    out.push(pages);
    return out;
  }

  // 表头（排序按钮）+ 筛选行 HTML（rant 2026-08-25T11:15:16：拆出独立渲染，整表重建不销毁筛选输入框）
  // 排序方向标记（" ▲" / " ▼" / ""）—— 表头构建与就地刷新共用的**唯一真源**。
  function sortArrow(state, key) {
    const sk = state.sort.find((s) => s.key === key);
    return sk ? (sk.dir === "asc" ? " ▲" : " ▼") : "";
  }

  // 列标题文案（列可声明 title 为函数：随语言变）。
  function colTitle(col) {
    return typeof col.title === "function" ? col.title() : col.title;
  }

  // 排序列头 ▲/▼ 就地刷新。为什么不能靠 tableTheadHtml：thead 只在容器无 <table> 时构建一次
  // （#148 为保住筛选输入框焦点），而排序状态是**此后**点击才产生的 ⇒ 表头不会自己重画，
  // 排序方向对用户永远不可见（仅在切语言触发的整表重建后才偶然出现一次）。
  // 就地改按钮文本而**不**重建 thead：重建会销毁筛选输入框，破坏 #148 的焦点不变量。
  function paintSortIndicators(container, columns, state) {
    const btns = container.querySelectorAll("thead [data-sort-key]");
    if (!btns.length) return;
    btns.forEach((b) => {
      const col = columns.find((c) => c.key === b.dataset.sortKey);
      if (!col) return;
      b.textContent = colTitle(col) + sortArrow(state, col.key);
    });
  }

  function tableTheadHtml(columns, state) {
    let html = "<tr>";
    columns.forEach((col) => {
      html += '<th' + (col.align === "num" ? ' class="num"' : "") + '><button type="button" class="th-sort" data-sort-key="' + esc(col.key) + '" title="' + T("tx.sort.title") + '">' +
        esc(colTitle(col)) + sortArrow(state, col.key) + "</button></th>";
    });
    html += "</tr><tr>";
    columns.forEach((col) => {
      const fv = state.filters[col.key] != null ? String(state.filters[col.key]) : "";
      if (col.filter === "select") {
        // 选项可为 {value, label}（value=语言无关的库内值，label=当前语言文案）或纯字符串（value==label）。
        // C2031：分离二者，筛选状态才能存库内值 —— 见 TX_COLUMNS 里 type/status 的注释
        const opts = (typeof col.options === "function" ? col.options() : (col.options || [])).map((o) => {
          const val = (o && typeof o === "object") ? o.value : o;
          const lbl = (o && typeof o === "object") ? o.label : o;
          return '<option value="' + esc(val) + '"' + (fv === String(val) ? " selected" : "") + ">" + esc(lbl) + "</option>";
        }).join("");
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
    html += "</tr>";
    return html;
  }

  // 数据行 HTML（每次重建 tbody 内容）
  function tableBodyHtml(pageRows, columns) {
    let html = "";
    if (!pageRows.length) html += '<tr><td colspan="' + columns.length + '" class="empty-cell">' + emptyState(T("tx.empty"), T("tx.empty.sub")) + "</td></tr>";
    pageRows.forEach((row) => {
      html += "<tr>";
      columns.forEach((col) => {
        html += "<td" + (col.align === "num" ? ' class="num"' : "") + ' data-label="' + esc(colTitle(col)) + '">' +
          (col.render ? col.render(row) : esc(row[col.key] == null ? "" : row[col.key])) + "</td>";
      });
      html += "</tr>";
    });
    return html;
  }

  function buildDataTable(cfg) {
    const { container, columns, rows, state, onState, serverPaging } = cfg;

    // 1) 筛选
    let data = filterRows(rows, columns, state.filters);

    // 2) 排序（多列：Shift 点击叠加）
    if (state.sort.length) {
      data = data.slice().sort((a, b) => {
        for (const sk of state.sort) {
          const col = columns.find((c) => c.key === sk.key);
          // C2054：列的**渲染值**可能与行里存的原始值不同（如 pts 的有符号值）——
          // 列可用 `sortVal` 声明「排序按哪个值」，缺省仍是 row[key]（其余 6 张表行为不变）。
          const av = col && col.sortVal ? col.sortVal(a) : a[sk.key];
          const bv = col && col.sortVal ? col.sortVal(b) : b[sk.key];
          let cmp;
          if (col && col.sort === "number") cmp = Number(av) - Number(bv);
          else cmp = String(av).localeCompare(String(bv), "zh-CN");
          if (cmp !== 0) return sk.dir === "asc" ? cmp : -cmp;
        }
        return 0;
      });
    }

    // 3) 分页（serverPaging：总数取后端 total，行即当前页——服务端已翻页，不做本地 slice）
    const totalRows = serverPaging ? serverPaging.total : data.length;
    const pages = Math.max(1, Math.ceil(totalRows / state.pageSize));
    if (state.page > pages) state.page = pages;
    const pageRows = serverPaging ? data : data.slice((state.page - 1) * state.pageSize, state.page * state.pageSize);

    // 4) 表头 + 筛选行：仅在容器无 <table> 时渲染一次（rant 2026-08-25T11:15:16：
    //    整表重建改为只重建数据行 + 分页器，筛选输入框 DOM 永不销毁 → 输入焦点天然保留；
    //    输入框值即 DOM 状态源，数据行重建读 state.filters，二者一致）
    //    C2031：切换语言时表头/下拉会「卡」在旧语言（列标题与选项文案都不会自己变），
    //    因此由 atp:langchange 处理器先移除 table 再重绘（见下方 rebuildDataTableHeader 调用点）。
    let table = container.querySelector("table");
    if (!table) {
      container.innerHTML = '<table class="table"></table>';
      table = container.querySelector("table");
      const thead = document.createElement("thead");
      thead.innerHTML = tableTheadHtml(columns, state); // 初值取 state.filters
      table.appendChild(thead);
      table.appendChild(document.createElement("tbody"));
      // 一次性容器级事件委托（排序 / 筛选输入 / 分页 / 每页行数 / IME 组合）——
      // 重建不重绑，事件不丢失；处理器读取 container._dt（每次渲染刷新）
      container.addEventListener("click", (e) => {
        const dt = container._dt; if (!dt) return;
        const sb = e.target.closest("[data-sort-key]");
        if (sb) {
          const key = sb.dataset.sortKey;
          const ex = dt.state.sort.find((s) => s.key === key);
          if (ex) {
            if (ex.dir === "asc") ex.dir = "desc";
            else dt.state.sort = dt.state.sort.filter((s) => s.key !== key);
          } else {
            if (!e.shiftKey) dt.state.sort = [];
            dt.state.sort.push({ key, dir: "asc" });
          }
          dt.state.page = 1;
          dt.onState();
          return;
        }
        const pb = e.target.closest("[data-p]");
        if (pb) { dt.state.page = Number(pb.dataset.p); dt.onState(); }
      });
      container.addEventListener("input", (e) => {
        const dt = container._dt; if (!dt) return;
        const el = e.target.closest(".th-filter");
        if (!el) return;
        const key = el.dataset.filterKey;
        const range = el.dataset.range;
        if (range) {
          const other = container.querySelector('[data-filter-key="' + key + '"][data-range="' + (range === "min" ? "max" : "min") + '"]');
          const min = range === "min" ? el.value : (other ? other.value : "");
          const max = range === "max" ? el.value : (other ? other.value : "");
          dt.state.filters[key] = min + ":" + max;
        } else {
          dt.state.filters[key] = el.value;
        }
        dt.state.page = 1;
        // rant 2026-08-23T16:01:07 Bug 2：逐字符 input 立即 onState() 会打断输入 → 300ms 防抖。
        // rant 2026-08-25T11:15:16：筛选行不再被销毁，焦点天然保留；恢复焦点仅兜底
        // （已聚焦时不动光标），IME 组合期间跳过（避免 setSelectionRange 打断中文输入法组合态）。
        if (el._dbTimer) clearTimeout(el._dbTimer);
        const focusSel = range ? '[data-filter-key="' + key + '"][data-range="' + range + '"]' : '[data-filter-key="' + key + '"]';
        el._dbTimer = setTimeout(() => {
          if (el._composing) return; // 组合中：不重建表格、不碰光标，待 compositionend 后再刷新
          dt.onState();
          const n = container.querySelector(focusSel);
          if (n && document.activeElement !== n) {
            n.focus();
            try { n.setSelectionRange(n.value.length, n.value.length); } catch (e2) { /* 非 text 元素忽略 */ }
          }
        }, 300);
      });
      container.addEventListener("change", (e) => {
        const dt = container._dt; if (!dt) return;
        const ps = e.target.closest("[data-page-size]");
        if (ps) { dt.state.pageSize = Number(ps.value); dt.state.page = 1; dt.onState(); }
      });
      container.addEventListener("compositionstart", (e) => {
        const el = e.target;
        if (el && el.classList && el.classList.contains("th-filter")) el._composing = true;
      });
      container.addEventListener("compositionend", (e) => {
        const el = e.target;
        if (!el || !el.classList || !el.classList.contains("th-filter")) return;
        el._composing = false;
        // 组合结束提交文本 → 防抖触发一次筛选刷新（覆盖组合期间的输入事件被跳过的情况）
        if (el._dbTimer) clearTimeout(el._dbTimer);
        el._dbTimer = setTimeout(() => {
          const dt = container._dt; if (!dt) return;
          dt.onState();
        }, 300);
      });
      // 组合被取消（Esc/切走）时清标志，避免 _composing 卡死导致后续输入不再刷新
      container.addEventListener("focusout", (e) => {
        const el = e.target;
        if (el && el.classList && el.classList.contains("th-filter")) el._composing = false;
      });
    }

    // 5) 数据行（每次重建 tbody 内容）
    const tbody = table.querySelector("tbody");
    tbody.innerHTML = tableBodyHtml(pageRows, columns);
    // 5b) 排序方向标记（thead 不重建，见 paintSortIndicators 注释）
    paintSortIndicators(container, columns, state);

    // 6) 分页器 + 每页行数（每次重建）
    const oldPager = container.querySelector(".pager");
    if (oldPager) oldPager.remove();
    if (pages > 1) {
      const pager = document.createElement("div");
      pager.className = "pager";
      let ph = "";
      pagerButtons(state.page, pages).forEach((p) => {
        if (p === "…") ph += '<span class="pager-ellipsis">…</span>';
        else ph += '<button type="button" class="' + (p === state.page ? "active" : "") + '" data-p="' + p + '">' + p + "</button>";
      });
      ph += "<span>" + state.page + " / " + pages + " · " + T("tx.pager.count", { n: totalRows }) + "</span>";
      pager.innerHTML = ph;
      container.appendChild(pager);
    }
    const oldPs = container.querySelector(".pager-size");
    if (oldPs) oldPs.remove();
    const psDiv = document.createElement("div");
    psDiv.className = "pager-size";
    psDiv.innerHTML = T("tx.pager.size") + ' <select data-page-size><option value="5">5</option><option value="10">10</option><option value="25">25</option><option value="50">50</option></select> ' + T("tx.pager.rows");
    const psSel = psDiv.querySelector("[data-page-size]");
    if (psSel) psSel.value = state.pageSize;
    container.appendChild(psDiv);

    // 7) 记录最新配置，供容器级事件委托读取
    container._dt = { state, onState };
  }

  // 语言切换时强制重建表头 + 筛选行（C2031）。
  // 为什么需要：buildDataTable 只在容器无 <table> 时建 thead（rant 2026-08-25T11:15:16 为保输入焦点），
  // 所以列标题（col.title()）与下拉选项文案（options().label）都是**一次性**的 —— 切语言后不会更新，
  // 表格会「一半英文一半中文」。这里移除 <table>，让下一次 buildDataTable 重新构建 thead。
  // 保留 #148 的性质：重建后 <select> 的值取 state.filters（即库内值）⇒ 用户已选的筛选**不丢**；
  // 文本筛选框同理由 state.filters 回填（焦点会丢，但切换语言本就是一次显式的全局操作）。
  function rebuildDataTableHeader(container) {
    if (!container) return;
    const table = container.querySelector("table");
    if (table) table.remove();
    const pager = container.querySelector(".pager");
    if (pager) pager.remove();
    const pagerSize = container.querySelector(".pager-size");
    if (pagerSize) pagerSize.remove();
  }

  /* --- 设置 --- */

  function renderSettings() {
    // 账户昵称框：真实昵称（rant 2026-08-22T00:01:52：不再静态写「阿零」，避免覆盖真实昵称）
    const nick = $("#settings-nickname");
    if (nick) nick.value = D.USER.name || (D.USER.email ? D.USER.email.split("@")[0] : "");
    // 邮箱为登录账号（/api/me 的真实邮箱，只读展示；无改邮箱后端接口）
    const mail = $("#settings-email");
    if (mail) mail.value = D.USER.email || "";
    // 接入端点卡片：实时从配置/同源 fallback 读取（rant 2026-08-19T20:37:37）
    applyEndpointUrls();
    renderPrefModels();
    const rawQ = $("#ak-search").value || "";
    const q = rawQ.toLowerCase();
    // 零 mock（rant 2026-08-19T15:54:06）：登录态绝不 fallback D.API_KEYS；
    // 加载失败 → 空态 + 重试；设置页仅登录可达
    if (loggedIn() && !Live.apiKeys) {
      setLiveError($("#api-keys"), loadErrorRow(6, T("settings.ak.loadFail"), T("err.loadFail")), () => loadApiKeys());
      pulseTbody($("#api-keys"));
      return;
    }
    // P2-B：登录 → 后端 /api/api-keys（key 已脱敏；完整 key 仅生成时可得）
    let list;
    if (Live.apiKeys) {
      // idx = 定位符：行内按钮带的索引是**缓存数组里的下标**，不是搜索过滤后的行号
      //（过滤会改变行号；copyKey/renameKey/deleteKey 都按 Live.apiKeys[idx] 取记录）
      list = Live.apiKeys.map((k, idx) => ({
        idx,
        id: k.id,
        fullKey: k.full_key || null,
        name: k.name || T("common.unnamed"),
        key: k.key,
        // C2126：「创建时间」列显示的是**本地日** —— 直接切服务端串的前 10 位拿到的是
        // UTC 日（东八区用户在当地 08:00 之前会看到昨天）。取 helper 产出的本地串的
        // 日期部分，而不是再抄一份「时间转字符串」的口径。
        created: fmtPrecise(k.created_at).slice(0, 10),
        last: k.last_used || null, // rant 2026-08-24T12:41:25：真实最近使用时间（NULL=从未使用）
        status: k.status || "active",
      }));
    } else {
      list = [];
    }
    list = list.filter((k) => !q || k.name.toLowerCase().includes(q));
    $("#api-keys").innerHTML = list.length ? list.map((k) =>
      "<tr data-key-row='" + k.idx + "'><td data-label='" + T('settings.ak.col.name') + "'><strong>" + hl(k.name, rawQ) + "</strong></td>" +
      "<td data-label='" + T('settings.ak.col.key') + "'><code>" + esc(Live.apiKeys ? k.key : "") + "</code></td>" +
      "<td data-label='" + T('settings.ak.col.created') + "'>" + esc(k.created) + "</td>" +
      "<td data-label='" + T('settings.ak.col.last') + "'>" + (k.last ? timeCell(k.last) : esc(T("settings.ak.last.never"))) + "</td>" +
      "<td data-label='" + T('settings.ak.col.status') + "'>" + (k.status === "active" ? '<span class="pill pill-ok">' + T("settings.ak.status.active") + "</span>" : '<span class="pill pill-muted">' + esc(k.status || "—") + "</span>") + "</td>" +
      "<td data-label='" + T('settings.ak.col.action') + "'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-copy='" + k.idx + "'>" + T("settings.ak.copy") + "</button> " +
      "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-key-rename='" + k.idx + "'>" + T("settings.ak.rename") + "</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-key-del='" + k.idx + "'>" + T("settings.ak.del") + "</button></td></tr>"
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
    // rant 2026-09-11T16:23:43（PR6 设置页）：原型拆三行端点 = 后端真实的三条路由
    // （src/routes/mod.rs：/v1/chat/completions、/v1/responses、/anthropic/v1/messages），
    // 不是虚标：OpenAI Chat 与 OpenAI Responses 共用 /v1 base，但协议路径不同
    return [
      { tag: () => T("settings.ep.openaiChat"), url: base + "/v1", desc: T("settings.ep.openaiChat.desc") },
      { tag: () => T("settings.ep.openaiResponses"), url: base + "/v1", desc: T("settings.ep.openaiResponses.desc") },
      { tag: () => T("settings.ep.anthropic"), url: base + "/anthropic", desc: T("settings.ep.anthropic.desc") },
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
    // 端点描述也随语言切换（原型把客户端列表写死在 DOM 里，线上走 i18n）
    document.querySelectorAll("[data-ep-desc]").forEach((el) => {
      const i = Number(el.getAttribute("data-ep-desc"));
      const ep = eps[i];
      if (ep) el.textContent = ep.desc;
    });
  }

  // 偏好「默认模型」下拉：真实模型列表（/api/models，零 mock；未加载时仅保留「未设置」）
  function renderPrefModels() {
    const sel = $("#prefs-model");
    if (!sel) return;
    const models = (Live.models || []).map((m) => m.model).filter(Boolean);
    const cur = sel.value;
    sel.innerHTML = '<option value="">' + esc(T("settings.prefs.model.none")) + "</option>" +
      models.map((m) => '<option value="' + esc(m) + '">' + esc(m) + "</option>").join("");
    if (cur && models.indexOf(cur) >= 0) sel.value = cur;
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
    // 行号会随搜索过滤变化 ⇒ 按定位符（缓存下标）找行，而不是 nth-child
    const row = document.querySelector('#api-keys [data-key-row="' + i + '"]');
    const cell = row ? row.children[0] : null;
    if (!cell) return;
    inlineForm(cell, {
      value: k.name,
      placeholder: T("settings.ak.rename.ph"),
      width: "160px",
      validate: (v) => v ? null : T("settings.ak.err.name"),
      // rant 2026-08-22T17:21:39：改名必须 PATCH 持久化（原实现只改内存变量 → 刷新失效假成功）
      onSubmit: (name) => {
        api.patch("/api/api-keys/" + k.id, { name }).then(async () => {
          await liveLoad("apiKeys", "/api/api-keys");
          renderSettings();
          toast(T("settings.ak.renamed", { name: name }), "success");
        }).catch((err) => {
          renderSettings(); // 失败回滚：重渲染还原真实名字
          toast((err && err.message) ? I18n.mapErr(err.message) : T("settings.ak.rename.fail"), "error");
        });
      },
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
        setLiveError($("#emp-body"), loadErrorRow(7, T("admin.emp.loadFail"), T("err.loadFail")), () => loadAdmin());
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
      // 成员搜索（原型 #emp-search：成员名 / 邮箱 / 部门；rant 16:23:43 第 9 节）
      const rawEmpQ = ($("#emp-search") && $("#emp-search").value) || "";
      const empQ = rawEmpQ.toLowerCase();
      const shown = users.filter((u) => !empQ ||
        (u.name || "").toLowerCase().includes(empQ) ||
        (u.email || "").toLowerCase().includes(empQ) ||
        (u.dept_name || "").toLowerCase().includes(empQ));
      $("#emp-body").innerHTML = shown.length ? shown.map((u) => {
        const i = users.indexOf(u);
        return "<tr data-emp-row='" + i + "'><td data-label='" + T('admin.emp.col.member') + "'><strong>" + hl(u.name || u.email, rawEmpQ) + "</strong>" +
        "<div class='muted' style='font-size:12px'>" + hl(u.email, rawEmpQ) + "</div></td>" +
        "<td data-label='" + T('admin.emp.col.role') + "'>" + (u.role === "admin" ? '<span class="pill pill-ok">admin</span>' : u.role === "ops" ? '<span class="pill pill-warn">ops</span>' : '<span class="pill pill-muted">user</span>') + "</td>" +
        "<td data-label='" + T('admin.emp.col.dept') + "'>" + (u.dept_name ? hl(u.dept_name, rawEmpQ) : '<span class="muted">' + T("common.unassigned") + "</span>") + "</td>" +
        '<td class="num" data-label="' + T("admin.emp.col.perm") + '">' + D.fmt(u.balance || 0) + "</td>" +
        '<td class="num" data-label="' + T("admin.emp.col.gift") + '">' + D.fmt(u.gift_balance || 0) + "</td>" +
        '<td class="num" data-label="' + T("admin.emp.col.avail") + '">' + D.fmt((u.balance || 0) + (u.gift_balance || 0)) + "</td>" +
        "<td data-label='" + T('admin.emp.col.action') + "'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-dept='" + i + "'>" + T("admin.emp.dept.change") + "</button> " +
        "<button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-emp-topup='" + i + "'>" + T("admin.emp.topup") + "</button></td></tr>";
      }).join("") : emptyRow(7, T("admin.emp.empty"), T("admin.emp.empty.sub"));
      pulseTbody($("#emp-body"));
      renderRaiseRequests();
    } else if (tab === "usage") {
      // 零 mock（rant 15:54:06）：用量报表仅登录可达，绝不 fallback D.USAGE_MODEL/D.USAGE_EMP
      if (!Live.adminUsage) {
        setLiveError($("#usage-model"), loadErrorHtml(T("admin.usage.loadFail"), T("err.loadFail")), () => loadAdmin());
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
      // 按部门（barRow 用 cost 归一）；无部门的桶后端回传空串（语言中性）⇒ 本地取语言包
      const maxDC = Math.max(1, ...depts.map((d) => d.cost || 0));
      $("#usage-dept").innerHTML = depts.length
        ? depts.map((d) => barRow(d.name || T("common.unassigned"), d.cost, maxDC, T("admin.usage.unit.yuan"))).join("")
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
      setLiveError($("#model-body"), loadErrorRow(8, T("admin.models.loadFail"), T("err.loadFail")), () => loadAdmin());
      pulseTbody($("#model-body"));
      return;
    }
    const rawQ = $("#model-search").value || "";
    const q = rawQ.toLowerCase();
    const list = Live.adminModels.filter((m) => !q ||
      (m.provider || "").toLowerCase().includes(q) || (m.model || "").toLowerCase().includes(q));
    // 定位符 = 缓存数组下标（editModelRow/deleteModel 按 Live.adminModels[i] 取记录）；
    // 搜索过滤会改变行号，所以不能把行号当定位符填进按钮
    $("#model-body").innerHTML = list.length ? list.map((m) => {
      const i = Live.adminModels.indexOf(m);
      return "<tr data-model-row='" + i + "'><td data-label='" + T('admin.models.col.provider') + "'><strong>" + esc(m.provider) + "</strong></td>" +
      "<td data-label='" + T('admin.models.col.model') + "'><code>" + esc(m.model) + "</code></td>" +
      '<td class="num" data-label="' + T("admin.models.col.in") + '">' + D.fmt(m.input_per_m || 0) + "</td>" +
      '<td class="num" data-label="' + T("admin.models.col.out") + '">' + D.fmt(m.output_per_m || 0) + "</td>" +
      '<td class="num" data-label="' + T("admin.models.col.ctx") + '">' + fmtCtx(m.context_length || m.context_window || 0) + "</td>" +
      '<td class="num" data-label="' + T("admin.models.col.outmax") + '">' + fmtCtx(m.max_output || 0) + "</td>" +
      "<td data-label='" + T('admin.models.col.vision') + "'>" + (m.vision ? '<span class="pill pill-ok">' + T("admin.models.vision.yes") + "</span>" : '<span class="pill pill-muted">' + T("admin.models.vision.no") + "</span>") + "</td>" +
      "<td data-label='" + T('admin.models.col.action') + "'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-model-edit='" + i + "'>" + T("admin.models.edit") + "</button> " +
      "<button class='btn btn-danger' style='padding:4px 10px;font-size:12px' data-model-del='" + i + "'>" + T("admin.models.del") + "</button></td></tr>";
    }).join("") : emptyRow(8, T("admin.models.empty"), T("admin.models.empty.sub"));
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
    // 忙碌态由点击监听器统一施加（`withLoading(e.currentTarget, confirmModel)`，与 confirmDept /
    // confirmRaise / confirmTopup 同款）。这里**不能**再嵌一层 withLoading：内层第一行的守卫
    // `if (!btn || btn.dataset.loading) return;` 会因外层已在点击瞬间置位而直接返回
    // ⇒ 新增/编辑模型的请求永不发出（#97 写下即死）。
    const req = editingId ? api.patch("/api/admin/models/" + editingId, body) : api.post("/api/admin/models", body);
    req.then(async () => {
      await loadAdmin();
      $("#model-form-card").hidden = true;
      toast(editingId ? T("admin.models.saved") : T("admin.models.added"), "success");
    }).catch((err) => {
      toast((err && err.message) ? I18n.mapErr(err.message) : T("admin.models.fail"), "error");
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
      setLiveError($("#dept-body"), loadErrorRow(7, T("admin.org.loadFail"), T("err.loadFail")), () => loadAdmin());
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

    // 定位符 = 缓存数组下标（openDeptForm/deleteDept 按 Live.departments[i] 取记录）；
    // 搜索过滤会改变行号，所以不能把行号当定位符填进按钮
    $("#dept-body").innerHTML = list.length ? list.map((d) => {
      const i = src.indexOf(d);
      const used = d.month_cost || 0;
      const members = d.member_count || 0;
      const pct = d.quota > 0 ? used / d.quota : 0;
      const st = pct >= 1 ? '<span class="pill pill-danger">' + T("common.exhausted") + "</span>" : pct > 0.9 ? '<span class="pill pill-warn">' + T("common.nearLimit") + "</span>" : '<span class="pill pill-ok">' + T("common.normal") + "</span>";
      return "<tr><td data-label='" + T('admin.org.col.dept') + "'><strong>" + hl(d.name, rawQ) + "</strong></td>" +
        '<td class="num" data-label="' + T("admin.org.col.members") + '">' + T("cnt.members", { n: members }) + "</td>" +
        '<td class="num" data-label="' + T("admin.org.col.quota") + '">' + D.fmt(d.quota) + " " + T("common.points") + "</td>" +
        '<td class="num" data-label="' + T("admin.org.col.used") + '">' + D.fmt(used) + " " + T("common.points") + "</td>" +
        '<td class="num" data-label="' + T("admin.org.col.remain") + '">' + D.fmt(d.quota - used) + " " + T("common.points") + "</td>" +
        "<td data-label='" + T('admin.org.col.status') + "'>" + st + "</td>" +
        "<td data-label='" + T('admin.org.col.action') + "'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-dept-edit='" + i + "'>" + T("common.edit") + "</button> " +
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
    return '<div class="bar-row"><div class="bar-top"><span>' + esc(name) + '</span><span class="n">' + D.fmt(pts) + " " + unit + "</span></div>" +
      '<div class="bar-track"><div class="bar-fill" style="width:' + pct + '%"></div></div></div>';
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
        setLiveError($("#ops-stats"), loadErrorHtml(T("ops.loadFail"), T("err.loadFail")), () => loadOps());
        $("#ops-hours").innerHTML = "";
        $("#ops-keys").innerHTML = "";
        return;
      }
      // P2-C：/api/ops/runtime 真实聚合
      const rt = Live.opsRuntime;
      $("#ops-stats").innerHTML = [
        stat(T("ops.stats.status"), '<span class="pill pill-ok">' + T("common.online") + "</span>", T("ops.stats.status.sub")),
        // 服务版本 / 运行时长：版本来自后端 env!("CARGO_PKG_VERSION")（与 /healthz 同源），
        // 绝不在前端写死——原型那张卡里的 "v0.7.20" 是一写就过期的字面量。
        stat(T("ops.stats.version"), "v" + esc(rt.version || ""), T("ops.stats.version.sub")),
        stat(T("ops.stats.uptime"), esc(fmtUptime(rt)), T("ops.stats.uptime.sub")),
        stat(T("ops.stats.users"), T("cnt.people", { n: rt.users }), T("ops.stats.users.sub")),
        stat(T("ops.stats.keys"), T("cnt.keys", { n: rt.active_keys }), T("ops.stats.keys.sub.on")),
        // 交易量：total_txs 自 85982e8（PR #80）起就一直在算、在返回（全库 COUNT(*)），
        // 但 v1.22 零 mock 重构（89963f3）删掉 mock 分支那一行时漏了重接——这是**回归**，
        // 不是新功能，也不是原型对齐（原型没有这张卡）。零 mock 不破：值取自响应，不取 D.TRANSACTIONS。
        stat(T("ops.stats.trades"), T("cnt.trades", { n: rt.total_txs }), T("ops.stats.trades.sub")),
        stat(T("ops.stats.calls"), T("cnt.calls", { n: rt.month_calls }), T("ops.stats.calls.sub")),
        stat(T("ops.stats.in"), "+" + D.fmt(rt.month_in) + " " + T("common.points"), T("ops.stats.in.sub")),
        stat(T("ops.stats.out"), "-" + D.fmt(rt.month_out) + " " + T("common.points"), T("ops.stats.out.sub")),
      ].join("");

      // 今日调用量（按小时）：后端 today_hours 为 0-23 全量补零数组（缺小时补 0，
      // 与交易页 txTrendDays 同款：GROUP BY 省略空桶会让柱子整体左移）
      const hours = (rt.today_hours || []).slice(0, 24);
      const maxH = Math.max(1, ...hours.map((h) => h.calls || 0));
      const hasHours = hours.some((h) => (h.calls || 0) > 0);
      $("#ops-hours").innerHTML = hasHours
        ? hours.map((h) => barRow(String(h.hour).padStart(2, "0") + ":00", h.calls || 0, maxH, T("cnt.calls.unit"))).join("")
        : '<div class="empty-state compact"><p>' + T("ops.hours.empty") + "</p></div>";

      // 上游 key 健康：按厂商聚合（健康 / N 个异常 / 全部失败 三态，对齐原型 .mini-list）
      const kh = rt.key_health || [];
      $("#ops-keys").innerHTML = kh.length ? kh.map((k) => {
        const total = k.total || 0;
        const off = k.off || 0;
        const pill = off === 0
          ? '<span class="pill pill-ok">' + T("ops.keys.healthy") + "</span>"
          : (off >= total
            ? '<span class="pill pill-danger">' + T("ops.keys.failed") + "</span>"
            : '<span class="pill pill-warn">' + T("ops.keys.abnormal", { n: off }) + "</span>");
        return '<div class="mini-item"><div><div class="t">' + esc(k.provider || "—") + '</div>' +
          '<div class="d">' + T("ops.keys.count", { total: total, on: k.on || 0 }) + "</div></div>" +
          '<div class="r">' + pill + "</div></div>";
      }).join("") : '<div class="empty-state compact"><p>' + T("ops.keys.empty") + "</p></div>";
      return;
    }

    // tab === "users"：成员充值（零 mock，rant 15:54:06）
    if (!Live.opsUsers) {
      setLiveError($("#ops-body"), loadErrorRow(4, T("ops.loadFail"), T("err.loadFail")), () => loadOps());
      pulseTbody($("#ops-body"));
      return;
    }
    const src = Live.opsUsers;
    const rawQ = $("#ops-search").value || "";
    const q = rawQ.toLowerCase();
    const list = src.filter((u) => !q || u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q));
    $("#ops-body").innerHTML = list.length ? list.map((u) =>
      "<tr><td data-label='" + T('ops.users.col.user') + "'><strong>" + hl(u.name, rawQ) + "</strong></td>" +
      "<td data-label='" + T('ops.users.col.email') + "'>" + hl(u.email, rawQ) + "</td>" +
      '<td class="num" data-label="' + T("ops.users.col.balance") + '">' + D.fmt(u.balance || 0) + " " + T("common.points") + "</td>" +
      "<td data-label='" + T('ops.users.col.action') + "'><button class='btn btn-ghost' style='padding:4px 10px;font-size:12px' data-ops-topup='" + u.id + "'>" + T("ops.users.topup") + "</button></td></tr>"
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

  // 运行时长（运营概览「运行时长」卡）：后端给分解好的 days/hours/minutes/secs_rest
  // （进位在后端做，前端只挑单位——两语言各写一遍进位没必要）。
  // 取最高两个非零位：天+小时 / 小时+分 / 分+秒 / 秒（刚启动）。
  function fmtUptime(rt) {
    const d = rt.uptime_days || 0;
    const h = rt.uptime_hours || 0;
    const m = rt.uptime_minutes || 0;
    const s = rt.uptime_secs_rest || 0;
    if (d > 0) return T("ops.uptime.days", { n: d }) + (h > 0 ? " " + T("ops.uptime.hours", { n: h }) : "");
    if (h > 0) return T("ops.uptime.hours", { n: h }) + (m > 0 ? " " + T("ops.uptime.minutes", { n: m }) : "");
    if (m > 0) return T("ops.uptime.minutes", { n: m }) + (s > 0 ? " " + T("ops.uptime.seconds", { n: s }) : "");
    return T("ops.uptime.seconds", { n: s });
  }

  // 精确本地时间（YYYY-MM-DD HH:MM:SS，到秒）——交易列表时间列（rant 2026-08-24T12:38:44）
  function fmtPrecise(s) {
    if (!s) return "";
    const t = String(s);
    const p2 = (x) => String(x).padStart(2, "0");
    let m = t.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?Z$/);
    let d = null;
    if (m) {
      d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +(m[6] || 0)));
    } else {
      // 旧后端格式 'YYYY-MM-DD[ HH:MM[:SS]]' → 视为 UTC（同 timeAgo 口径）
      m = t.match(/^(\d{4})-(\d{2})-(\d{2})(?:\s+(\d{2}):(\d{2})(?::(\d{2}))?)?$/);
      if (m) d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +(m[4] || 0), +(m[5] || 0), +(m[6] || 0)));
    }
    if (d && !isNaN(d.getTime())) {
      return d.getFullYear() + "-" + p2(d.getMonth() + 1) + "-" + p2(d.getDate()) +
        " " + p2(d.getHours()) + ":" + p2(d.getMinutes()) + ":" + p2(d.getSeconds());
    }
    return t; // 非标准格式（游客 mock 'MM-DD HH:mm' 等）原样返回
  }

  // 时间单元格：precise=true → 精确本地时间为主文本、相对时间入 title（交易列表，rant 2026-08-24T12:38:44）；
  // 默认 → 相对时间为主文本、title 悬停显示本地化绝对时间（rant 2026-08-19T20:45:32）
  function timeCell(s, precise) {
    if (!s) return "";
    const t = String(s);
    const iso = /^(\d{4})-(\d{2})-(\d{2})[T ]/.test(t);
    let title = t;
    if (iso) {
      const d = new Date(iso && t.includes("T") ? t : t.replace(" ", "T") + "Z");
      if (!isNaN(d.getTime())) title = d.toLocaleString();
    }
    if (precise) {
      const rel = timeAgo(s);
      return '<span class="timeago" title="' + esc(rel) + '">' + esc(fmtPrecise(s)) + "</span>";
    }
    return '<span class="timeago" title="' + esc(title) + '">' + esc(timeAgo(s)) + "</span>";
  }

  function openChat(id) {
    // 零 mock（rant 15:54:06）：登录态绝不回退 D.MARKET
    if (loggedIn() && !Live.models) { toast(T("err.loadFail"), "error"); return; }
    const m = (marketRows() || []).find((x) => modelKey(x) === id);
    if (!m) return;
    if (!m.avail) { toast(T("chat.busy"), "error"); return; }
    markRecentUsed(modelKey(m)); // 记录最近使用（rant 20:46:57 D：去重 + 置顶，最多 5 个）
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
    const m = src.find((x) => modelKey(x) === id);
    if (!m) return;
    if (!m.avail) { toast(T("chat.busy"), "error"); return; }
    if (!loggedIn()) { toast(T("chat.login.need"), "error"); return; }
    markRecentUsed(modelKey(m)); // 记录最近使用（rant 20:46:57 D）
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

  // 游客模式：只隐藏账号相关控件（用户 chip + 退出登录），主题切换保持可用（原位于顶栏，PR2 迁入底栏）
  function setGuestSidebar(on) {
    const chip = document.querySelector(".user-chip");
    const out = $("#logout-btn");
    if (chip) chip.classList.toggle("hidden", on);
    if (out) out.classList.toggle("hidden", on);
  }

  function enterGuest() {
    isGuest = true;
    pendingHashView = null;
    activeView = "marketplace";
    $("#login-view").classList.add("hidden");
    $("#app").classList.remove("hidden");
    setGuestSidebar(true);
    renderNav();
    switchView("marketplace");
    toast(T("guest.enter"), "info");
  }

  function exitGuest() {
    isGuest = false;
    // 身份边界（C2132）：会话结束即清空上一位用户的缓存 —— 否则下一位登录者会先看到他的数据
    resetSessionCaches();
    $("#app").classList.add("hidden");
    setGuestSidebar(false);
    $("#login-view").classList.remove("hidden");
  }

  /* ---------------- 会话（P2-A：/api/me + /api/wallet） ---------------- */

  // 拉当前用户信息 + 钱包余额（替代 mock D.USER.*）；余额失败 → 0 + 红色提示
  async function loadSession() {
    // 身份边界（C2132）：会话建立前清空缓存 —— 登出再登录时，各视图的同步首帧会先渲染
    // 上一位用户的载荷（`renderView` 先同步渲染、再异步拉取）。
    resetSessionCaches();
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
    // 设置页「偏好 → 默认模型」下拉与市场同源（/api/models）；此处理不阻塞会话建立
    try { await liveLoad("models", "/api/models"); } catch (e) { Live.models = null; }
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
    setGuestSidebar(false);
    $("#login-view").classList.add("hidden");
    $("#app").classList.remove("hidden");
    $("#side-balance").textContent = D.fmt(D.USER.balance);
    renderUserChip();
    renderNav();
    // URL hash 路由：登录后恢复刷新前的视图（无 hash 则仪表盘）
    switchView(viewFromHash() || "dashboard");
    maybeStartTour(); // 首次登录引导（rant 20:46:57 A：atp-tour-done 未标记才触发）
  }

  // 会话恢复（boot 唯一入口；rant 2026-09-14T21:15:02 第 4 条）。
  //
  // 判定分三档，只有第一档能把用户判成「未登录」：
  //   · 401        → token 已失效：api.js 已清 token 并回登录页（`__atpLogout`）。**唯一**
  //                  不重试的情形 —— 重试不会让一个失效 token 变有效。
  //   · 可重试失败 → 网络错误（`status === 0`）/ 5xx（含网关 504）：等 1s 重试**一次**。
  //                  抖动通常只持续数百毫秒，一次重试即可救回，不必惊动用户。
  //   · 其余失败   → 4xx（非 401）：重试无意义。
  //
  // ⚠️ 重试后仍失败 ⇒ **照常进入 app**，绝不把用户摆在登录页。token 仍在（`api.getToken()`
  // 非空）却显示登录页 = 谎报「已登出」，且 URL hash 仍指向上次视图，用户只会理解为被踢出
  // （宿主 2026-09-14 21:00 实测：`/api/me` 被拖到网关 504 时三特征同现）。两个状态必须分开：
  // 「加载失败」由各视图自己的降级态（`loadErrorHtml`/`loadErrorRow` + 重试）承担，
  // 「未登录」只由 401 路径承担。若 token 其实已失效，进入后第一次真实请求会拿到 401，
  // 由 api.js 清 token 回登录页 —— 那条路径给出的是诚实的「登录已过期」。
  async function restoreSession() {
    for (let attempt = 0; ; attempt++) {
      try {
        await loadSession();
        enterApp();
        return true;
      } catch (e) {
        const status = (e && e.status) || 0;
        if (status === 401) return false; // 已由 api.js 处理（清 token + 回登录页）
        const transient = status === 0 || status >= 500;
        if (!transient || attempt >= 1) {
          enterApp();
          toast(T("login.session.fail"), "error");
          return true;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
  }

  // api.js 401 钩子：token 失效 → 清 token 回登录页
  window.__atpLogout = () => {
    api.clearToken();
    exitGuest();
    toast(T("login.session.expired"), "error");
  };

  /* ---------------- P2-B 真实 API 数据层（各视图 mock 数据逐步替换为后端） ---------------- */

  // 仪表盘双色柱图窗口天数（rant 2026-09-11T16:23:43 第 3 节：原型为固定 14 列）
  const DASH_TREND_DAYS = 14;

  // 各视图真实数据缓存：登录且加载成功后使用；游客 / 失败降级 mock
  const Live = {
    publicUrl: null,     // GET /api/config → public_url（接入端点 base，rant 2026-08-19T20:37:37）
    models: null,        // GET /api/models 原始数组
    plans: null,         // GET /api/plans 原始数组（上架表单数据源；rant 16:14:21 Bug 1）
    sharings: null,      // GET /api/sharings 原始数组
    transactions: null,  // GET /api/transactions → {items,total,...}（**交易视图自己的**载荷，唯一写者 loadTransactions）
    tradeCount: null,    // 仪表盘「交易笔数」：GET /api/transactions?page_size=1 → total（C2130：仪表盘自己的槽，**不是**上面那个缓存）
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

  // 身份边界（C2132）：`Live` 是**按会话**缓存，必须在**会话结束**（exitGuest：登出 / 401）
  // 与**会话建立**（loadSession：boot / 登录）两侧都清空 —— 否则换账号后，各视图会先用
  // 上一位用户的载荷渲染（`renderView` 先同步渲染、再异步拉取）。钱包视图尤其致命：它是
  // 八个视图里**唯一**没有自己的 loader 的分支，缓存不被清就永远显示上一个人的「永久点数」。
  // 槽名**派生自**上面那个对象字面量（新增槽自动纳入）—— 不维护第二份名册。
  function resetSessionCaches() {
    Object.keys(Live).forEach((k) => { Live[k] = null; });
  }

  function loggedIn() { return !!api.getToken() && !isGuest; }

  // 拉取并缓存；失败抛错（调用方决定降级）
  async function liveLoad(key, path) {
    const data = await api.get(path);
    Live[key] = data;
    return data;
  }

  // 通用降级渲染：加载失败 → 空态 + 重试按钮（不白屏）
  function loadErrorHtml(emptyLabel, retryLabel) {
    return '<div class="empty-state">' + EMPTY_ICON +
      "<p>" + esc(emptyLabel) + "</p>" +
      '<p class="muted">' + esc(retryLabel || T("err.loadFail")) + '</p>' +
      '<button type="button" class="btn btn-ghost" data-live-retry>' + T("common.retry") + "</button></div>";
  }
  // 「加载失败 → 重试」的容器级委托：渲染方调用 setLiveError(容器, html, loader) 时把 loader 一并交出来，
  // 容器上的**一次性** click 委托（重建不重绑）再把 `[data-live-retry]` 的点击分发给它。
  // 谁渲染降级态谁就负责传 loader ⇒ 新增表格不必在任何名册里登记（漏登记 = 重试按钮点了没反应）。
  const liveLoaders = new WeakMap();     // 容器 → 该容器的重试回调
  const liveRetryBound = new WeakSet();  // 已挂上委托的容器（只挂一次）
  function setLiveError(container, html, loader) {
    if (!container) return;
    liveLoaders.set(container, loader);
    if (!liveRetryBound.has(container)) {
      liveRetryBound.add(container);
      container.addEventListener("click", (e) => {
        if (!e.target.closest || !e.target.closest("[data-live-retry]")) return;
        const fn = liveLoaders.get(container);
        if (fn) fn();
      });
    }
    container.innerHTML = html;
  }

  // tbody 容器专用降级行：<tr><td> 内嵌 loadErrorHtml（div 直接进 tbody 会被浏览器提升到表外，破坏布局与重试委托）
  function loadErrorRow(colspan, emptyLabel, retryLabel) {
    return '<tr><td class="empty-cell" colspan="' + colspan + '">' + loadErrorHtml(emptyLabel, retryLabel) + "</td></tr>";
  }

  // 后端 models → 视图行（点数按 points_per_unit=1、锚定 CNY 折算；USD 价 ×7.2；ctx 来自 models.context_window；
  // multi=available_keys>=2 真实计算；success 后端暂无字段 → null，视图不渲染假成功率；
  // peak 高峰时段价（rant 2026-08-20T11:58:40）：peak_input_per_m>0 → 启用高峰计费，展示 ×N 标注）
  // 零 mock（rant 2026-08-19T15:54:06）：不读 data.js MARKET 兜底
  // 模型行的**稳定身份**（C2138）：`provider/model`。**位置不是身份** —— `modelsToView()` 曾用
  // `id: i`（数组下标）当模型的身份，而下标只在生成它的那一次渲染里有意义：`/api/models` 按
  // `provider, model` 排序（加/删一个模型就让后面全部位移），游客兜底表 `data.js > MARKET` 更是
  // 另一张表（7 行、id 1..7、顺序与长度都不同，只是**数字上看起来**是同一个空间）。
  // 一旦下标被存进 localStorage（「最近使用」），它就跨了渲染 / 跨了会话 / 跨了数组 —— 芯片于是
  // 指向**另一个模型**（实测：用了 xai/grok-4.6，游客市场里显示 google/gemini-3.1-pro），或者
  // 因为对不上号而**整条消失**（登录态下标从 0 起，游客表 id 从 1 起）。
  function modelKey(m) { return (m && m.provider ? m.provider : "") + "/" + (m && m.model ? m.model : ""); }

  function modelsToView(list) {
    return list.map((m) => {
      const cny = m.currency === "CNY";
      const mult = cny ? 1 : 7.2;
      const peak = (m.peak_input_per_m || 0) > 0;
      return {
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
        // rant 2026-09-11T16:23:43 第 4 节：可用性 pill 需显示真实 key 数、能力标签需 vision 字段
        keys: m.available_keys || 0,
        vision: !!m.vision,
        success: null,
        live: true,
      };
    });
  }

  // 后端 sharings → 视图行（字段对齐 mock：earned=earn、price 用 autoPrice、time=上架时间）
  // 「本月新增」的**月份口径**在这里算，且只算一次（见 utcMonth）：
  // 服务端返回的 created_at 是 UTC ISO，全站聚合口径也都是 UTC（ops/wallet/admin/org），
  // 所以卡片不能改用本地月份——那会在每月 1 日 00:00–08:00（UTC+8）与同页其它数字不一致。
  function utcMonth(iso) {
    if (!iso || iso.length < 7) return "";
    return iso.slice(0, 7);
  }

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
        time: s.created_at || "",
        month: utcMonth(s.created_at),
        note: s.note || "",
        available: days.length ? { days, start: s.available_start || "", end: s.available_end || "" } : null,
      };
    });
  }

  // 后端 transactions items → 视图行（模型 / Key 两列 + Token 四列平铺；rant 2026-08-22T06:36:54/06:37:50/06:39:04）
  // rant 2026-08-22T08:58:54：Token 数字加 K/M 缩写，悬停显示精确值
  const fmtTokens = (n) => {
    if (typeof n !== "number" || !(n > 0)) return "0";
    if (n >= 1e6) return (n / 1e6).toFixed(2) + "M";
    if (n >= 1000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "K";
    return String(Math.round(n));
  };
  const fmtTokensExact = (n) => (typeof n === "number" ? String(Math.round(n)) : "0");
  function txsToView(items) {
    return items.map((t) => {
      let tokens = "—";
      if (typeof t.tokens === "number" && t.tokens > 0) {
        tokens = fmtTokens(t.tokens);
      }
      // Token 四列：输入(非缓存) / 缓存 / 输出 / 总；旧记录（cached/output=0）输入=总
      const inputTokens = fmtTokens(typeof t.input_tokens === "number" ? t.input_tokens : null);
      const cachedTokens = fmtTokens(typeof t.cached_tokens === "number" ? t.cached_tokens : null);
      const outputTokens = fmtTokens(typeof t.output_tokens === "number" ? t.output_tokens : null);
      // 悬停显示精确值（K/M 缩写下的全量）
      const brkTitle = (label, v) => {
        const exact = typeof v === "number" && v > 0 ? fmtTokensExact(v) : "0";
        return ' title="' + esc(label + ": " + exact) + '"';
      };
      // 模型列 / Key 列（rant 2026-08-22T17:21:39 需求 2）：优先库内值 ——
      // 模型列取 `t.model`；Key 列优先分发 key 的 name（api_keys.name），历史行无 api_key_id
      // → 兜底 key_label（note/provider/plan）。
      // C2113：**无值时只能用服务端同样搜得到的占位符**。两列的筛选都走后端（`tx_where` 的
      // `model` / `key_name` LIKE），故单元格里出现的每一段文字都必须是该列筛选能命中的值；
      // 此前无值行显示的是**本地化类型名**（`txType(t.type)`，如「赠送」）—— 服务端拿不到语言包，
      // 那个文案永远匹配不到 ⇒ 按屏幕上刚看到的文案筛选 0 行（客户端与「类型」列都有正确的控件，
      // 所以这不是丢功能，是**说谎的漏斗**：文案看着可筛选、实际 0 行）。占位符 `—` 与
      // `user` 列（`t.user_name || "—"`）及 Key 列消费分支同款：**无值、且语言中性**，
      // 服务端筛选表达式尾部的 `'—'` 与这里逐字对应（见 `src/routes/wallet.rs` `tx_where`）。
      const model = t.model || "—";
      const key = t.key_name || t.key_label || "—";
      return {
        id: t.id,
        // C2126：视图行**原样**带出服务端串，格式化交给渲染器（列是 `timeCell(..., true)`
        // → `fmtPrecise`，CSV 导出用同一个 helper）。此前在这里先切/替换成
        // `YYYY-MM-DD HH:MM`，秒位被抹掉，而两处渲染的口径都是 `HH:MM:SS`
        // ⇒ 屏幕上的秒数永远是伪造的 `00`（C2111 修好的是「导出与单元格同口径」，
        // 两份口径同源之后，源头的截断就成了唯一的口径）。
        time: t.time || "",
        type: t.type,
        // 用户列（rant 2026-08-22T17:21:39 需求 2）：transactions.user_id JOIN users 取用户名
        user: t.user_name || "—",
        model,
        key,
        detail: t.model ? "消费 · " + t.model : "交易",
        tokens,
        tokensRaw: typeof t.tokens === "number" ? t.tokens : null,
        inputTokens,
        inputRaw: typeof t.input_tokens === "number" ? t.input_tokens : null,
        cachedTokens,
        cachedRaw: typeof t.cached_tokens === "number" ? t.cached_tokens : null,
        outputTokens,
        outputRaw: typeof t.output_tokens === "number" ? t.output_tokens : null,
        tokenBrk: brkTitle,
        pts: t.pts,
        status: t.status || "成功",
      };
    });
  }

  // 本月点数变化（`#dash-month-changes` / `#month-changes`）那一格的**唯一**写者。
  //
  // 这一格被**两个**视图渲染：仪表盘 `renderDashboard` 与钱包 `renderWallet` 都经 `renderMonthChanges`
  // 读 `Live.dashboard` ⇒ 它是**共享**槽，写者只能有一个（C2131：一个槽两个写者会让「缓存」与
  // 「它的有效性证据」脱钩）。装载它的每个视图各调一次即可 —— 这正是 C2135 的修法：
  // 此前 `renderWallet` 渲染这一格，而它的 loader 只刷 `Live.wallet`，于是会话若在钱包视图上
  // 建立（hash `#/wallet` 后登录 / 在钱包页登出再登录），这一格永远印「本月暂无变动」。
  async function refreshDashboard() {
    try { Live.dashboard = await api.get("/api/dashboard"); }
    catch (e) { Live.dashboard = null; }
  }

  // 刷新钱包缓存（登录后）；返回最新 available
  async function refreshWallet() {
    try {
      Live.wallet = await api.get("/api/wallet");
      if (typeof Live.wallet.available === "number") D.USER.balance = Live.wallet.available;
      return Live.wallet.available;
    } catch (e) { return D.USER.balance; }
  }

  // 钱包视图自己的 loader（C2132）：`renderView("wallet")` 此前只渲染不拉取，是八个视图里
  // **唯一**没有 loader 的分支 —— 缓存一旦有值（哪怕是上一位用户的）就永远不会刷新。
  // C2135：渲染谁就装载谁 —— 钱包页同样渲染「本月点数变化」，它的数据在 `Live.dashboard` 里，
  // 只拉钱包的话那一格在「会话建立于钱包视图」时永远是空的（详见 refreshDashboard 的注释）。
  async function loadWallet() {
    await refreshWallet();
    await refreshDashboard();
    renderWallet();
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
        // 这个端点的 401 是「凭据不对」，不是「会话失效」——必须声明（C2120）：否则 api.js
        // 会清 token 并发一句「登录已过期，请重新登录」，与下面的行内报错同时出现在屏幕上，
        // 而用户从没登录过。
        const r = await api.post("/api/auth/login", { email, password: pass }, { on401: api.CREDENTIAL_401 });
        api.saveToken(r.api_key);
        // 凭据已被接受 ⇒ 此后与 boot 走**同一个**入口（`restoreSession` 的三档判定）：
        // 只有 401 意味未登录，网络/5xx 重试一次，其余非 401 照常进 app。
        // 绝不把刚拿到 token 的用户留在登录页 —— token 已存却显示登录页 = 谎报「已登出」。
        if (await restoreSession()) {
          toast(T("login.welcome", { name: D.USER.name || email }), "success");
        }
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
      // 登录专属附加项（分隔线/游客入口/底部提示）随登录表单一起显隐
      const extrasEl = $("#auth-login-extras");
      if (extrasEl) extrasEl.classList.toggle("hidden", which !== "login");
      // 登录/注册 tab 高亮同步（rant 2026-09-11T16:23:43）；验证码/找回页 tab 全部取消高亮
      const tabs = document.querySelectorAll("#auth-tabs button");
      for (let i = 0; i < tabs.length; i++) {
        tabs[i].classList.toggle("active", tabs[i].dataset.auth === which);
      }
      // 标题固定为「进入平台」（原型 .login-card h2；tab 只切换表单区）
      const tabBar = $("#auth-tabs");
      if (tabBar) tabBar.classList.toggle("hidden", which === "verify" || which === "forgot");
    }

    // tab 点击 → 复用既有 showAuthForm（注册流程零改动）
    const authTabsEl = $("#auth-tabs");
    if (authTabsEl) {
      authTabsEl.addEventListener("click", (e) => {
        const b = e.target.closest("button[data-auth]");
        if (!b) return;
        const which = b.dataset.auth;
        showAuthForm(which);
        const focusEl = which === "register" ? $("#reg-email") : $("#login-email");
        if (focusEl) focusEl.focus();
      });
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
    // 可用性筛选（rant 2026-09-11T16:23:43 第 4 节：all / 仅可用）
    if ($("#mk-avail")) $("#mk-avail").addEventListener("change", renderMarketplace);

    // 市场页：使用 / 消费（G4：聊天 Mock 扣小数点数并产生 consume 交易；游客需先登录 US-1）
    $("#mk-body").addEventListener("click", (e) => {
      // 行展开 / 收起（rant 20:39:30 F：仅展开当前行，点其它行自动收起）
      const ex = e.target.closest("[data-mk-expand]");
      if (ex) {
        const id = ex.dataset.mkExpand;
        mkExpanded = mkExpanded === id ? null : id;
        renderMarketplace();
        return;
      }
      const b = e.target.closest("[data-use-model]");
      if (b) {
        if (isGuest) { toast(T("chat.login.need"), "error"); return; }
        consumeModel(b.dataset.useModel);
        return;
      }
      // 空状态：清除筛选
      if (e.target.closest("[data-mk-clear-filters]")) {
        resetSearch($("#mk-search"));
        $("#mk-provider").value = "";
        $("#mk-sort").value = "default";
        if ($("#mk-avail")) $("#mk-avail").value = "all";
        renderMarketplace();
      }
    });
    // 最近使用 chips（rant 20:46:57 D：点击直接使用 / 清空）
    $("#mk-recent").addEventListener("click", (e) => {
      if (e.target.closest("[data-mk-recent-clear]")) {
        saveRecentKeys([]);
        renderRecent();
        return;
      }
      const c = e.target.closest("[data-recent-model]");
      if (c) {
        if (isGuest) { toast(T("chat.login.need"), "error"); return; }
        openChat(c.dataset.recentModel);
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
          const label = provLabel(plan.provider) + " · " + planLabel(plan);
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
    // Enter 走容器级委托（C2127）：卡片里每个文本控件都生效，且新增字段不用再登记。
    wireEnterSubmit($("#topup-card"), "#topup-confirm");
    wireEnterSubmit($("#raise-card"), "#raise-confirm");
    ["topup-card", "raise-card"].forEach((id) => {
      document.getElementById(id).addEventListener("keydown", (e) => {
        if (e.key === "Escape") { document.getElementById(id).hidden = true; }
      });
    });
    $$("#topup-card .topup-presets .chip").forEach((b) =>
      b.addEventListener("click", () => {
        $$("#topup-card .topup-presets .chip").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
        $("#topup-custom").value = "";
      })
    );
    // 钱包页提示 → 跳转交易记录（明细统一入口）
    $("#wallet-goto-tx").addEventListener("click", () => switchView("transactions"));

    // 仪表盘 page-head 动作按钮（rant 2026-09-11T16:23:43 第 3 节：去市场 / 管理共享）
    // 容器级委托，避免为每个视图各绑一次；与原型 data-goto 语义一致
    $$("[data-goto]").forEach((b) => b.addEventListener("click", () => switchView(b.dataset.goto)));

    // 交易 Tab（P2-B：切 tab 重新拉后端过滤数据）：tab 写的就是类型筛选本身（setTxTypeFilter）
    // —— 它也负责把「类型」列筛选控件同步成同一个值，否则两份控件又会各说各话；控件状态一变，
    // renderTransactions 里的签名比对自会重拉（勿再显式调 loadTransactions，会与它并发两次请求）。
    $$("#tx-tabs .tab").forEach((b) => b.addEventListener("click", () => {
      setTxTypeFilter(b.dataset.txTab === "all" ? "" : b.dataset.txTab);
      txTable.page = 1;
      renderTransactions();
    }));
    $("#tx-export-btn").addEventListener("click", exportTxCsv); // 导出 CSV（rant 20:46:57 E）

    // 交易时间段（rant 2026-08-22T10:50:00：快捷范围 + 自定义起止，切换后重载列表与汇总）
    const txRangeEl = $("#tx-range");
    const txStartEl = $("#tx-range-start");
    const txEndEl = $("#tx-range-end");
    if (txRangeEl && txStartEl && txEndEl) {
      const showCustom = () => {
        const custom = txRangeEl.value === "custom";
        txStartEl.style.display = custom ? "" : "none";
        txEndEl.style.display = custom ? "" : "none";
      };
      txRangeEl.addEventListener("change", () => {
        txRange = txRangeEl.value;
        showCustom();
        txTable.page = 1;
        renderTransactions();
        if (loggedIn()) loadTransactions();
      });
      txStartEl.addEventListener("change", () => { txCustomStart = txStartEl.value; txTable.page = 1; if (loggedIn()) loadTransactions(); });
      txEndEl.addEventListener("change", () => { txCustomEnd = txEndEl.value; txTable.page = 1; if (loggedIn()) loadTransactions(); });
      showCustom();
    }

    // 趋势图：原型 .trend 双色柱状（消费/收益）为纯 CSS 柱 + title 原生 tooltip，
    // 无需 JS 绑定（旧 SVG 折线图的指标切换/悬停定位/动态 viewBox 随实现一并移除）。
    // 仅保留 resize 重渲染（窄屏时柱宽由 flex 自适应，重渲染保证标签密度与数据一致）
    const txTrendEl = $("#tx-trend");
    if (txTrendEl) {
      let _txTrendResizeT = null;
      window.addEventListener("resize", () => {
        if (txTrendEl.offsetParent === null) return;
        clearTimeout(_txTrendResizeT);
        _txTrendResizeT = setTimeout(renderTxTrend, 120);
      });
    }

    // 表格键盘导航（rant 20:46:57 F）：点击行 → 激活高亮，之后 ↑/↓/Enter/Esc 可用。
    // document 级一次性委托 + 容器从 DOM 派生 ⇒ 所有数据表自动可导航（新增表格不必登记，
    // 动态重建的表格也不会丢绑定）。
    document.addEventListener("click", (e) => {
      const c = kbdTbodyOf(e.target);
      if (!c) return;
      const tr = e.target.closest ? e.target.closest("tr") : null;
      if (!tr || (tr.classList && tr.classList.contains("mk-detail"))) return;
      kbdSet(c, kbdRows(c).indexOf(tr));
    });

    // API Key 生成（行内编辑；列表展示脱敏、复制给完整 id）
    $("#new-api-key-btn").addEventListener("click", openNewKeyInline);
    $("#ak-new-ok").addEventListener("click", commitNewKey);
    $("#ak-new-cancel").addEventListener("click", closeNewKeyInline);
    // Enter 提交走容器级委托（C2127）；Esc 收起由容器自己管。
    wireEnterSubmit($("#ak-new-inline"), "#ak-new-ok");
    $("#ak-new-inline").addEventListener("keydown", (e) => { if (e.key === "Escape") closeNewKeyInline(); });

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

    // 成员搜索（原型 #emp-search：成员 / 邮箱 / 部门；rant 2026-09-11T16:23:43 第 9 节）
    wireSearch($("#emp-search"), renderAdmin);

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
    // 键盘可达（B.9）：部门表单 Enter 提交、Esc 收起。Enter 走容器级委托（C2127）。
    wireEnterSubmit($("#dept-form-card"), "#dept-confirm");
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
    // Enter 提交由容器级委托统一提供（C2127）：这里原先是**逐字段**登记，而名册只写了
    // 「厂商」「模型名」两个 —— 输入价 / 缓存命中价 / 输出价 / 高峰三价 / 上下文窗口 /
    // 最大输出这 8 个控件按 Enter 毫无反应（点确认都能提交）。
    wireEnterSubmit($("#model-form-card"), "#model-confirm");
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
    // C2136：boot **不渲染任何视图** —— 此处会话尚未建立（`restoreSession()` 在下面才跑），
    // 而 `renderView` 是「渲染 + 装载」：带 token 时 `loggedIn()` 此刻已为 true，于是仪表盘
    // 那一整套查询会在**会话还不存在**时就发出去（实测 `log[0] = GET /api/wallet`，
    // 会话请求 `/api/me` 才排第 2），随后被 `loadSession()` 的 `resetSessionCaches()` 作废，
    // 再由 `enterApp() → switchView(目的地)` 重新装一遍 ⇒ 一次 boot 里仪表盘那套**各发两次**；
    // 目的地不是仪表盘时（如刷新在 `#/transactions`）更白拉一整屏数据。过期 token 时更糟：
    // 先发的那 6 个请求每个都拿到 401，用户会看到多条一模一样的「登录已过期」。
    // 视图只由 `switchView` 渲染/装载它**当前的目的地**；boot 只管外壳（导航 / 事件 / 余额占位）。
    // 登录态加载失败的空态重试按钮（rant 2026-08-19T15:48:17 / 15:54:06）：由渲染方经 setLiveError
    // 把 loader 交给容器，这里不再逐个 id 登记。
    $("#side-balance").textContent = D.fmt(D.USER.balance);

    // P2-A 会话恢复：已有 token → 拉 /api/me + /api/wallet 直接进 app。
    // 失败处置集中在 restoreSession()：401 回登录页（唯一「未登录」信号），
    // 其余失败重试一次后照常进 app 并提示 —— 不把「加载失败」演成「已登出」。
    (async () => {
      if (!api.getToken()) return;
      await restoreSession();
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
      // C2031：数据表的表头/筛选行是一次性构建的（见 rebuildDataTableHeader），切语言时先拆除，
      // 由随后的 renderView → renderTransactions 重建成当前语言；筛选值存在 state 里，不会丢失。
      rebuildDataTableHeader($("#tx-table"));
      if (activeView) renderView(activeView);
      document.title = (VIEW_TITLE[activeView] ? T(VIEW_TITLE[activeView]) + " · AITokenPool" : "AITokenPool");
      if (tourStep >= 0) renderTourStep(); // 引导中的按钮/文案随语言更新
    });

    // 主题切换（rant 18:06:09 B）：登录页右上角 + 侧边栏底部两处共用同一逻辑
    function toggleTheme() {
      const next = document.documentElement.dataset.theme === "light" ? "dark" : "light";
      applyTheme(next);
      toast(T("theme.switched", { theme: next === "light" ? T("theme.light") : T("theme.dark") }), "info");
    }
    // 应用主题（toggleTheme 与设置页「偏好 → 主题」下拉共用；localStorage atp-theme 记忆）
    function applyTheme(next) {
      document.documentElement.dataset.theme = next;
      const sel = $("#prefs-theme");
      if (sel) sel.value = next;
      try { localStorage.setItem("atp-theme", next); } catch (e) { /* 隐私模式忽略 */ }
    }
    $("#theme-toggle").addEventListener("click", toggleTheme);
    const loginThemeBtn = $("#login-theme-toggle");
    if (loginThemeBtn) loginThemeBtn.addEventListener("click", toggleTheme);
    // 偏好「主题」下拉（原型设置页 .pref-theme；与右上/侧边栏切换按钮同源）
    const themeSel = $("#prefs-theme");
    if (themeSel) {
      themeSel.value = document.documentElement.dataset.theme || "dark";
      themeSel.addEventListener("change", () => applyTheme(themeSel.value));
    }

    // 全局快捷键（rant 16:57:17 D）：/ 聚焦市场搜索；数字键切换侧边栏视图（1..NAV_ORDER.length）；Esc 关闭行内新建 key
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
      // 数字键切换视图：不再写死上限——角标是 NAV_ORDER 下标 +1，这里用同一数组取项，
      // 越界数字自然落空，视图增删后两者自动保持一致（原先写死 "7"，插入 ops 后 settings 角标成了 8）
      if (/^[0-9]$/.test(e.key)) {
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
