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
      throw { status: 0, message: "网络不可用，请检查后端服务是否启动" };
    }
    if (resp.status === 401) {
      handleUnauthorized();
      throw { status: 401, message: "登录已过期，请重新登录" };
    }
    const text = await resp.text();
    let data = null;
    if (text) {
      try { data = JSON.parse(text); } catch (e) { data = { raw: text }; }
    }
    if (!resp.ok) {
      // 取后端 error.message 或 error 字段
      const message = (data && (data.error && (data.error.message || data.error))) || (data && data.message) || ("请求失败（HTTP " + resp.status + "）");
      const err = new Error(message);
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
