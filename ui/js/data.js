/* ============================================================
   AITokenPool UI — 内嵌数据（rant 2026-08-19T15:54:06：登录态零 mock）
   保留对象仅用于：游客市场渲染（MARKET）与上架表单兜底（MODELS/PLANS/PROVIDERS/PROVIDER_LABELS）；
   登录态一律使用后端 API（ui/js/api.js），绝不读取下列 mock 数据。
   点数规则与机制细节见 docs/user-stories.md（v1.8：机制不进 UI）
   ============================================================ */

(function () {
  "use strict";

  // CNY 模型：1 元 = 1 点；USD 模型：按 ~7.2 汇率折算为人民币点数（仅游客/表单兜底展示用；
  // 真实计价以后端 billing 的 anchor_currency × points_per_unit 为准）
  const USD = (usd) => Math.round(usd * 7.2);
  const CNY = (cny) => Math.round(cny);

  // 模型价格（对齐 config.toml [[models]] 官方价，折算为点数 / 1M tokens）——上架表单定价兜底
  const MODELS = [
    { provider: "deepseek", model: "deepseek-v4-pro",  in: CNY(4.5),   out: CNY(13.5),  ctx: 1048576, max: 384000, tag: "推理" },
    { provider: "deepseek", model: "deepseek-v4-flash", in: CNY(1.5),   out: CNY(4.5),   ctx: 1048576, max: 384000, tag: "通用" },
    { provider: "zhipu",    model: "glm-5.2",          in: CNY(8.0),   out: CNY(28.0),  ctx: 1048576, max: null,   tag: "通用" },
    { provider: "openai",   model: "gpt-5.5-pro",      in: USD(30.0),  out: USD(180.0), ctx: 1050000, max: null,   tag: "旗舰" },
    { provider: "anthropic",model: "claude-opus-4.7",   in: USD(5.0),   out: USD(25.0),  ctx: 1000000, max: null,   tag: "旗舰" },
    { provider: "google",   model: "gemini-3.1-pro",   in: USD(2.0),   out: USD(12.0),  ctx: 1048576, max: null,   tag: "多模态" },
    { provider: "xai",      model: "grok-4.6",         in: USD(2.0),   out: USD(6.0),   ctx: 500000,  max: null,   tag: "通用" },
    { provider: "moonshot", model: "kimi-k3",          in: CNY(20.0),  out: CNY(100.0), ctx: 1048576, max: null,   tag: "长文" },
    { provider: "bytedance-ark", model: "doubao-seed-2.1-pro", in: CNY(6.0), out: CNY(30.0), ctx: 262144, max: null, tag: "通用" },
    { provider: "minimax",  model: "minimax-m3",       in: USD(0.3),   out: USD(1.2),   ctx: 1048576, max: null,   tag: "轻量" },
    { provider: "aliyun-bailian", model: "qwen3-max",  in: CNY(4.0),   out: CNY(16.0),  ctx: 262144,  max: null,   tag: "通用" },
  ];

  // 金额/余额格式化：整数原样（千分位）；小数保留 2 位（v1.6 CNY 锚定，消费点数可为小数）
  const fmt = (n) => Number.isInteger(n) ? n.toLocaleString("zh-CN") : n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const ctxFmt = (n) => (n >= 1000000 ? (n / 1000000).toFixed(1) + "M" : fmt(n));

  window.ATDATA = {
    fmt, ctxFmt,

    MODELS,

    PROVIDERS: [...new Set(MODELS.map((m) => m.provider))],

    // Plan 清单（与 config/config.example.toml 的 [[plans]] id 对齐 —— 上架表单 payload.plan
    // 直接提交给后端，id 必须一致；rant 2026-08-18T16:14:21 Bug 1）
    // type: paygo=按量计价 key | coding=编码订阅 | token=统一计量订阅（Credits）
    // 登录后 renderSharing 优先用 GET /api/plans 覆盖此清单（后端单一真源）
    PLANS: [
      { id: "deepseek-paygo",      provider: "deepseek",       name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "无订阅 plan，仅按量；OpenAI/Anthropic/Responses 端点" },
      { id: "zhipu-coding",        provider: "zhipu",          name: "GLM Coding Plan", type: "coding", keyPrefix: "",      note: "专属端点 /api/coding/paas/v4、/api/anthropic" },
      { id: "zhipu-paygo",         provider: "zhipu",          name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "open.bigmodel.cn 按量" },
      { id: "aliyun-token-plan",   provider: "aliyun-bailian", name: "Token Plan",      type: "token",  keyPrefix: "sk-sp-", note: "Credits 统一计量，专属 key sk-sp-，多工具通用" },
      { id: "aliyun-bailian-paygo",provider: "aliyun-bailian", name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "dashscope 兼容模式按量" },
      { id: "ark-coding",          provider: "bytedance-ark",  name: "Coding Plan",     type: "coding", keyPrefix: "",      note: "/api/coding/v3、/api/coding" },
      { id: "ark-agent",           provider: "bytedance-ark",  name: "Agent Plan",      type: "token",  keyPrefix: "",      note: "/api/plan/v3、/api/plan" },
      { id: "ark-paygo",           provider: "bytedance-ark",  name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "按量计价" },
      { id: "kimi-code",           provider: "moonshot",       name: "Kimi Code 会员",  type: "coding", keyPrefix: "",      note: "api.kimi.com/coding/v1、/coding" },
      { id: "moonshot-paygo",      provider: "moonshot",       name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "api.moonshot.cn 按量" },
      { id: "minimax-coding",      provider: "minimax",        name: "Coding Plan",     type: "coding", keyPrefix: "sk-cp-", note: "专属 key sk-cp-，/anthropic、/v1" },
      { id: "minimax-paygo",       provider: "minimax",        name: "API（按量）",     type: "paygo",  keyPrefix: "sk-",    note: "按量计价" },
    ],

    // 厂商显示名（上架表单 / 共享列表）——安全兜底，登录态可用（rant 15:54:06 豁免）
    PROVIDER_LABELS: {
      "aliyun-bailian": "阿里云百炼",
      zhipu: "智谱",
      "bytedance-ark": "火山方舟",
      moonshot: "Kimi 月之暗面",
      minimax: "MiniMax",
      deepseek: "DeepSeek",
    },

    // 当前用户会话存储：登录后由 /api/me 覆盖 name/email/role、/api/wallet 覆盖 balance
    // （初始值仅占位，登录态永不渲染——rant 2026-08-19T15:54:06 豁免）
    USER: { name: "访客", email: "", role: "user", balance: 0 },

    // 市场在售 key（游客浏览用；rant 2026-08-19T15:54:06：multi/success 为虚构数据已移除；
    // 登录态用 GET /api/models 真实数据，multi=available_keys>=2、ctx=context_window）
    MARKET: [
      { id: 1, provider: "deepseek", model: "deepseek-v4-flash", in: USD(0.14), out: USD(0.28), ctx: 1048576, avail: true, peak: true, peakIn: USD(0.28), peakOut: USD(0.56), peakMult: 2 },
      { id: 2, provider: "zhipu",    model: "glm-5.2",           in: CNY(8.0),  out: CNY(28.0),  ctx: 1048576, avail: true },
      { id: 3, provider: "openai",   model: "gpt-5.5-pro",       in: USD(30.0), out: USD(180.0), ctx: 1050000, avail: true },
      { id: 4, provider: "anthropic",model: "claude-opus-4.7",    in: USD(5.0),  out: USD(25.0),  ctx: 1000000, avail: false },
      { id: 5, provider: "google",   model: "gemini-3.1-pro",    in: USD(2.0),  out: USD(12.0),  ctx: 1048576, avail: true },
      { id: 6, provider: "moonshot", model: "kimi-k3",           in: CNY(20.0), out: CNY(100.0), ctx: 1048576, avail: true },
      { id: 7, provider: "xai",      model: "grok-4.6",          in: USD(2.0),  out: USD(6.0),   ctx: 500000,  avail: true },
    ],
  };
})();
