/* AITokenPool — API 客户端层（P2-A，rant 2026-08-18T11:49:52）
 *
 * 封装 fetch：api.get / api.post / api.patch，自动带 Bearer token；
 * 统一错误处理：401 → 清 token 回登录页；非 2xx → 抛 {status, message}。
 * base URL：默认同源（''），支持 ?api= 查询参数覆盖（部署时指向网关）。
 */
const api = (() => {
  // base URL 覆盖：?api=https://gateway.example.com
  const base = (() => {
    try {
      const q = new URLSearchParams(window.location.search).get("api");
      return q ? q.replace(/\/+$/, "") : "";
    } catch (e) {
      return "";
    }
  })();

  // 记住我 → token 存 localStorage（长期）；否则 sessionStorage（关闭即失效）
  const rememberKey = "atp-remember";
  const tokenKey = "atp_token";

  function isRemember() {
    try { return localStorage.getItem(rememberKey) === "1"; } catch (e) { return false; }
  }

  function saveToken(token) {
    const s = isRemember() ? localStorage : sessionStorage;
    try {
      s.setItem(tokenKey, token);
      // 双写清理：确保另一 storage 无残留（从记住切到不记住等场景）
      (isRemember() ? sessionStorage : localStorage).removeItem(tokenKey);
    } catch (e) { /* 隐私模式忽略 */ }
  }

  function getToken() {
    try {
      return (isRemember() ? localStorage : sessionStorage).getItem(tokenKey) || "";
    } catch (e) {
      return "";
    }
  }

  function clearToken() {
    try { localStorage.removeItem(tokenKey); } catch (e) { /* ignore */ }
    try { sessionStorage.removeItem(tokenKey); } catch (e) { /* ignore */ }
  }

  // 401 → 清 token 回登录页（登出态；调用方无需重复处理）
  function handleUnauthorized() {
    clearToken();
    if (window.__atpLogout) window.__atpLogout();
  }

  // 本文件**自己的**错误文案一律按 key 取，不写中文原文（C2029）。
  //
  // 为什么必须这样：`src/i18n_pack.rs` 的三条门禁只读 i18n.js / index.html / app.js，
  // **不读本文件**；而本文件的匹配形态「mapErr + 中文原文」也不是门禁认得的「T + 键字面量」。
  // 因此在这里写中文原文 = 同时逃过「键存在 / 键被正确使用 / 占位符」三道断言，
  // 且 en 模式下会直接把中文抛给用户（实测 8 种真实后端响应形态里 6 种如此）。
  //
  // ⚠️ 必须是**函数声明**，不能写成 `const T = window.t`（app.js 的写法）：index.html 里
  // 本文件在 i18n.js **之前**加载，模块级捕获会永久绑到 undefined；函数声明则被提升，
  // 且每次调用才现取，与加载顺序无关。
  // ⚠️ 也不能直接调 `window.t(...)`：那是 app.js 的局部别名，全局并不存在。
  // 真正稳定的全局是 i18n.js 导出的 `window.I18n`。
  // ⚠️ 刻意沿用 `T` 这个名字：`src/i18n_pack.rs` 的键扫描器只认这一种调用形态，
  // 换个名字就得再写一份识别规则 —— 规则一旦分叉，两条门禁统计的就不再是同一批调用点（坑 75）。
  function T(key, vars) {
    if (window.I18n) return window.I18n.t(key, vars);
    return window.t ? window.t(key, vars) : key;
  }

  async function request(method, path, body) {
    const headers = { "content-type": "application/json" };
    const token = getToken();
    if (token) headers.authorization = "Bearer " + token;
    let resp;
    try {
      resp = await fetch(base + path, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (e) {
      throw { status: 0, message: T("err.network") };
    }
    if (resp.status === 401) {
      handleUnauthorized();
      throw { status: 401, message: T("login.session.expired") };
    }
    const text = await resp.text();
    let data = null;
    if (text) {
      try { data = JSON.parse(text); } catch (e) { data = { raw: text }; }
    }
    if (!resp.ok) {
      // 取后端 error.message 或 error 字段
      // 后端文案是**未知散文**，只能过 mapErr 词表；本文件自己的兜底文案走 tr()，
      // 因为 mapErr 以 `t(key)`（无 vars）收尾，带 `{n}` 的值经它只会原样输出花括号。
      const raw = (data && (data.error && (data.error.message || data.error))) || (data && data.message);
      const errMsg = raw ? (window.I18n ? window.I18n.mapErr(raw) : raw) : T("err.http", { n: resp.status });
      const err = new Error(errMsg);
      err.status = resp.status;
      throw err;
    }
    return data;
  }

  return {
    base,
    get: (path) => request("GET", path),
    post: (path, body) => request("POST", path, body),
    patch: (path, body) => request("PATCH", path, body),
    del: (path) => request("DELETE", path),
    saveToken,
    getToken,
    clearToken,
    isRemember,
  };
})();
