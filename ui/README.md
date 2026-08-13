# AITokenPool UI 原型（Static HTML Prototype）

纯静态 HTML + CSS + JS 原型，无框架、无构建工具、无外部 CDN 依赖。
浏览器直接打开即可浏览与演示。

## 如何浏览

**方式一（推荐）**：双击 `index.html`，用浏览器打开即可。

**方式二**：本地起一个静态服务器（可选，避免 file:// 下的个别限制）：

```bash
python3 -m http.server 8000 --directory ui
# 然后访问 http://localhost:8000
```

## 页面清单

登录页（统一入口，占位）：

- 邮箱登录 + 企业 SSO 占位按钮
- 登录前可切换 公共版 / 企业版，进入对应演示模式

公共版（共享市场）：

1. 仪表盘 Dashboard — 点数余额、本月用量、共享收益、最近交易
2. 模型市场 Marketplace — 模型浏览、搜索、厂商筛选、排序（按价格/上下文）
3. 共享管理 Sharing — 上架 key 表单（可填写并提交）、我的共享列表（暂停/恢复/重新上架）
4. 钱包 Wallet — 点数余额、充值/提现占位入口、收支明细（分页）
5. 交易记录 Transactions — 消费/收益/充值/提现，Tab 筛选 + 分页
6. 设置 Settings — 账户、API Key 管理（可生成）、通知、偏好

企业版（Admin / Enterprise）：

7. 企业管理台 Admin — Key 池管理（添加/撤销）、员工管理（配额调整）、用量报表（按模型/员工）、组织设置
8. 员工自助面板 Employee Portal — 我的点数、可用模型、申请加额（占位）

## 文件结构

```
ui/
├── index.html        # 入口（登录页 + 应用外壳 + 全部视图）
├── css/style.css     # 设计系统（深色主题 · 强调色 #4ecdc4 · 响应式预留）
├── js/data.js        # 内嵌 mock 数据（模型价格、交易、共享、员工等）
├── js/app.js         # 交互逻辑（导航、筛选、表单、分页、Toast）
└── README.md         # 本文件
```

## 数据说明

- 点数规则：1 USD = 1000 点；模型价格折算自 `data/models.example.json`（CNY 按 ~7.2 汇率示例折算）
- 全部数据为前端常量（mock），无后端、无真实调用
