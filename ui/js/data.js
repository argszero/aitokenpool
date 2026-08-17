/* ============================================================
   AITokenPool UI Prototype — 内嵌 mock 数据
   点数规则与机制细节见 docs/user-stories.md（v1.8：机制不进 UI）
   ============================================================ */

(function () {
  "use strict";

  // CNY 模型：1 元 = 1 点；USD 模型：按 ~7.2 汇率折算为人民币点数
  const USD = (usd) => Math.round(usd * 7.2);
  const CNY = (cny) => Math.round(cny);

  // 模型价格（来自 data/models.example.json，折算为点数 / 1M tokens）
  const MODELS = [
    { provider: "deepseek", model: "deepseek-v4-pro",  in: USD(0.435), out: USD(0.87),  ctx: 1048576, max: 384000, tag: "推理" },
    { provider: "deepseek", model: "deepseek-v4-flash", in: USD(0.14),  out: USD(0.28),  ctx: 1048576, max: 384000, tag: "通用" },
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

  // 可用性（市场展示用）
  MODELS.forEach((m) => {
    m.availability = Math.random() < 0.85 ? "available" : "busy";
  });
  MODELS[2].availability = "available"; // glm
  MODELS[4].availability = "busy";      // claude 演示"繁忙"

  // 金额/余额格式化：整数原样（千分位）；小数保留 2 位（v1.6 CNY 锚定，消费点数可为小数）
  const fmt = (n) => Number.isInteger(n) ? n.toLocaleString("zh-CN") : n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const ctxFmt = (n) => (n >= 1000000 ? (n / 1000000).toFixed(1) + "M" : fmt(n));

  window.ATDATA = {
    fmt, ctxFmt,

    MODELS,

    PROVIDERS: [...new Set(MODELS.map((m) => m.provider))],

    // Plan 清单（内置国内已知 Plan，源自 docs/plan-api-matrix.md 与 config/config.example.toml）
    // type: api=按量计价 key | coding=编码订阅 | token=统一计量订阅（Credits）
    // 每个厂商至少含一个 type=api 的「API（按量）」plan；models 按 provider 关联 MODELS
    PLANS: [
      { id: "aliyun-api",    provider: "aliyun-bailian", name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "按量计价" },
      { id: "aliyun-token",  provider: "aliyun-bailian", name: "Token Plan",      type: "token",  keyPrefix: "sk-sp-",  note: "Credits 统一计量，专属 key sk-sp-，多工具通用" },
      { id: "zhipu-api",     provider: "zhipu",          name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "open.bigmodel.cn 按量" },
      { id: "zhipu-coding",  provider: "zhipu",          name: "GLM Coding Plan", type: "coding", keyPrefix: "sk-",    note: "专属端点 /api/coding/paas/v4、/api/anthropic" },
      { id: "ark-api",       provider: "bytedance-ark",  name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "按量计价" },
      { id: "ark-coding",    provider: "bytedance-ark",  name: "Coding Plan",     type: "coding", keyPrefix: "sk-",    note: "/api/coding/v3、/api/coding" },
      { id: "ark-agent",     provider: "bytedance-ark",  name: "Agent Plan",      type: "token",  keyPrefix: "sk-",    note: "/api/plan/v3、/api/plan" },
      { id: "kimi-api",      provider: "moonshot",       name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "api.kimi.com 按量" },
      { id: "kimi-code",     provider: "moonshot",       name: "Kimi Code 会员",  type: "coding", keyPrefix: "sk-",    note: "api.kimi.com/coding/v1、/coding" },
      { id: "minimax-api",   provider: "minimax",        name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "按量计价" },
      { id: "minimax-coding",provider: "minimax",        name: "Coding Plan",     type: "coding", keyPrefix: "sk-cp-", note: "专属 key sk-cp-，/anthropic、/v1" },
      { id: "deepseek-api",  provider: "deepseek",       name: "API（按量）",     type: "api",    keyPrefix: "sk-",    note: "无订阅 plan，仅按量；OpenAI/Anthropic/Responses 端点" },
    ],

    // 厂商显示名（上架表单 / 共享列表）
    PROVIDER_LABELS: {
      "aliyun-bailian": "阿里云百炼",
      zhipu: "智谱",
      "bytedance-ark": "火山方舟",
      moonshot: "Kimi 月之暗面",
      minimax: "MiniMax",
      deepseek: "DeepSeek",
    },

    // 当前用户（公共版）
    USER: { name: "阿零", email: "demo@aitokenpool.local", balance: 12471 },

    // 加额申请（US-20：企业成员申请 → 管理员批准 / 驳回；mock 内嵌数据）
    RAISE_REQUESTS: [
      { id: 1, user: "苏航", email: "suhang@example.com", amount: 5000, reason: "本月开发任务增加，配额已用尽", status: "pending", time: "08-15 09:00" },
    ],

    // 交易记录（消费 / 收益 / 充值 / 提现 / 赠送）——唯一明细数据源，钱包页与交易记录页共用此表
    TRANSACTIONS: [
      { id: 13, time: "08-14 09:00", type: "gift",    partner: "—",                 detail: "赠送 · 每日赠送 +1", tokens: "—",  pts: +1,   status: "入账" },
      { id: 1, time: "08-13 20:15", type: "consume", partner: "deepseek-v4-flash", detail: "消费 · 用 shared key", tokens: "0.19M", pts: -0.37, status: "成功" },
      { id: 2, time: "08-13 19:02", type: "earn",    partner: "glm-5.2",           detail: "收益 · 我的 key 被消费", tokens: "860K", pts: +378, status: "入账" },
      { id: 3, time: "08-13 17:44", type: "consume", partner: "gpt-5.5-pro",       detail: "消费 · 用 shared key", tokens: "45K",  pts: -860, status: "成功" },
      { id: 4, time: "08-13 15:10", type: "earn",    partner: "kimi-k3",           detail: "收益 · 我的 key 被消费", tokens: "1.5M", pts: +882, status: "入账" },
      { id: 5, time: "08-13 11:36", type: "topup",   partner: "—",                 detail: "充值", tokens: "—",       pts: +5000, status: "成功" },
      { id: 6, time: "08-12 21:08", type: "consume", partner: "gemini-3.1-pro",    detail: "消费 · 用 shared key", tokens: "2.1M", pts: -310, status: "成功" },
      { id: 7, time: "08-12 14:22", type: "earn",    partner: "deepseek-v4-flash", detail: "收益 · 我的 key 被消费", tokens: "3.0M", pts: +189, status: "入账" },
      { id: 8, time: "08-12 09:41", type: "consume", partner: "claude-opus-4.7",   detail: "消费 · 用 shared key", tokens: "22K",  pts: -640, status: "成功" },
      { id: 9, time: "08-11 18:30", type: "withdraw", partner: "—",                detail: "提现（占位）", tokens: "—",       pts: -2000, status: "处理中" },
      { id: 10, time: "08-11 10:12", type: "earn",   partner: "glm-5.2",           detail: "收益 · 我的 key 被消费", tokens: "700K", pts: +351, status: "入账" },
      { id: 11, time: "08-10 16:05", type: "consume", partner: "doubao-seed-2.1-pro", detail: "消费 · 用 shared key", tokens: "540K", pts: -260, status: "成功" },
      { id: 12, time: "08-10 08:50", type: "earn",   partner: "minimax-m3",        detail: "收益 · 我的 key 被消费", tokens: "2.4M", pts: +162, status: "入账" },
    ],

    // 我的共享（price = 按模型定价自动计算的输出单价 点数/1M；earned = 累计收益；key 仅脱敏展示）
    // provider + plan：上架时选择「厂商 → Plan → 模型」，列表展示「厂商 · Plan」
    // available：结构化可用时间段（days 星期 1-7 + start/end 起止；null = 全天不限）
    SHARINGS: [
      { id: 1, provider: "zhipu", plan: "GLM Coding Plan", model: "glm-5.2", quota: 100000, used: 32800, price: 28, earned: 1611, status: "on", time: "08-17 09:30", key: "sk-zhipu-9f2c41ab7d22", available: { days: [1, 2, 3, 4, 5], start: "09:00", end: "18:00" } },
      { id: 2, provider: "deepseek", plan: "API（按量）", model: "deepseek-v4-flash", quota: 80000, used: 51200, price: 2, earned: 1116, status: "on", time: "08-16 20:15", key: "sk-ds-3b88d077aa91", available: null },
      { id: 3, provider: "moonshot", plan: "Kimi Code 会员", model: "kimi-k3", quota: 50000, used: 800, price: 100, earned: 54, status: "paused", time: "08-15 14:00", key: "sk-ms-c31f2e8b4405", available: { days: [6, 7], start: "10:00", end: "22:00" } },
      { id: 4, provider: "minimax", plan: "Coding Plan", model: "minimax-m3", quota: 20000, used: 20000, price: 9, earned: 432, status: "off", time: "08-10 11:20", key: "sk-mx-77d2a1c9e356", available: null },
    ],

    // 市场在售 key（公共版浏览；multi=true 表示该模型配置多个上游 key → 自动故障转移，架构 v0.2 路由策略 G5）
    MARKET: [
      { id: 1, provider: "deepseek", model: "deepseek-v4-flash", in: USD(0.14), out: USD(0.28), ctx: 1048576, avail: true,  success: 99.2, multi: true },
      { id: 2, provider: "zhipu",    model: "glm-5.2",           in: CNY(8.0),  out: CNY(28.0),  ctx: 1048576, avail: true,  success: 98.6, multi: true },
      { id: 3, provider: "openai",   model: "gpt-5.5-pro",       in: USD(30.0), out: USD(180.0), ctx: 1050000, avail: true,  success: 97.1, multi: true },
      { id: 4, provider: "anthropic",model: "claude-opus-4.7",    in: USD(5.0),  out: USD(25.0),  ctx: 1000000, avail: false, success: 95.4, multi: false },
      { id: 5, provider: "google",   model: "gemini-3.1-pro",    in: USD(2.0),  out: USD(12.0),  ctx: 1048576, avail: true,  success: 98.9, multi: false },
      { id: 6, provider: "moonshot", model: "kimi-k3",           in: CNY(20.0), out: CNY(100.0), ctx: 1048576, avail: true,  success: 96.8, multi: false },
      { id: 7, provider: "xai",      model: "grok-4.6",          in: USD(2.0),  out: USD(6.0),   ctx: 500000,  avail: true,  success: 93.7, multi: false },
    ],

    // 我的 API Keys（设置页；存完整 id，展示时脱敏，复制时给完整值）
    API_KEYS: [
      { id: "atk_live_9f2c1a7b4d22e41b", name: "本地 CLI 工具", created: "2026-08-01", last: "08-13 20:11", status: "active" },
      { id: "atk_live_3b88d077aa91c357", name: "CI / 自动化", created: "2026-07-22", last: "08-12 15:40", status: "active" },
    ],

    // 平台运营者视图（US-运营1 / US-运营2）：公共场景注册用户列表（mock）
    // 运营者 = 宿主本人，职责仅两项：查看运行概览 + 给指定用户充值（永久有效点数，产生交易记录）
    // 注意：id=1 即当前演示账号「阿零」，运营者对其充值时同步更新 D.USER.balance
    OPERATOR_USERS: [
      { id: 1, name: "阿零",   email: "demo@aitokenpool.local", balance: 12471 },
      { id: 2, name: "陈默",   email: "chenmo@example.com",     balance: 320 },
      { id: 3, name: "林小满", email: "linxm@example.com",      balance: 860 },
      { id: 4, name: "苏航",   email: "suhang@example.com",     balance: 45.5 },
    ],

    // —— 企业版 ——

    // 员工（dept 可为空 = 未分配部门，如新注册未安排）
    EMPLOYEES: [
      { id: 1, name: "陈默", dept: "研发", quota: 20000, used: 12400 },
      { id: 2, name: "林小满", dept: "研发", quota: 20000, used: 19850 },
      { id: 3, name: "苏航", dept: "产品", quota: 15000, used: 3200 },
      { id: 4, name: "周雨", dept: "市场", quota: 10000, used: 9800 },
      { id: 5, name: "何明", dept: "设计", quota: 8000,  used: 2100 },
      { id: 6, name: "赵欣", dept: "", quota: 20000, used: 3500 },  // 新注册 · 未分配
      { id: 7, name: "王磊", dept: "", quota: 15000, used: 0 },     // 新注册 · 未分配
    ],

    // 部门（组织管理 — 每月点数分配；已用/成员数由 EMPLOYEES 实时汇总，保持联动）
    DEPARTMENTS: [
      { id: 1, name: "研发", quota: 80000 },
      { id: 2, name: "产品", quota: 30000 },
      { id: 3, name: "市场", quota: 20000 },
      { id: 4, name: "设计", quota: 15000 },
    ],

    // 用量报表
    USAGE_MODEL: [
      { name: "gpt-5.5-pro", pts: 18600 },
      { name: "deepseek-v4-flash", pts: 13200 },
      { name: "glm-5.2", pts: 9800 },
      { name: "claude-opus-4.7", pts: 7400 },
      { name: "gemini-3.1-pro", pts: 3100 },
    ],
    USAGE_EMP: [
      { name: "林小满", pts: 19850 },
      { name: "陈默", pts: 12400 },
      { name: "周雨", pts: 9800 },
      { name: "苏航", pts: 3200 },
      { name: "何明", pts: 2100 },
    ],
  };
})();
