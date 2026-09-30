//! 停机预算门禁（rant 2026-09-30T16:25:29）：**容器给的停机窗口必须比应用自己的排水上限长**。
//!
//! 起因：`main()` 里 `/healthz` 与业务路由原本是裸 `axum::serve(listener, app)` —— 没有
//! SIGTERM 处理、没有在途请求排水 ⇒ 停容器的瞬间在途请求被直接掐断（连接重置）。修法是
//! 让应用收到停机信号后**停止接受新连接、等待在途请求完成**，并给它一个上限
//!（`main.rs::DRAIN_LIMIT_SECS`）—— 上限之内排完就是优雅停机，排不完（SSE / 长连接）
//! 就强制退出，这是「有上限」的必然代价。
//!
//! 但**应用自己排不完不代表 docker 会等它**：`docker stop` / Swarm 更新 / `compose up -d`
//! 先发 SIGTERM，然后按 `stop_grace_period` 计时，到点就 SIGKILL。于是同一件事有了
//! **两个**数、**两个**载体：
//!
//! | 载体 | 值 | 语义 |
//! |---|---|---|
//! | `src/main.rs` 的 `DRAIN_LIMIT_SECS` | 应用 | 收到信号后最多等多久在途请求 |
//! | `docker-compose.yml` 的 `stop_grace_period` | 容器 | SIGTERM 之后多久 SIGKILL |
//!
//! 二者**必须满足 `stop_grace_period > DRAIN_LIMIT_SECS`**：反过来的话 docker 会在应用
//! 还在排水时把它打死，优雅停机被自己的上限削掉一截、退化回原来的症状，而且**没有任何
//! 报错**（docker 只是按配置执行）。这与 `deploy_gate.rs` 守的版本副本、`smtp_port_gate.rs`
//! 守的端口同形：**跨格式的副本能抄，前提是有人守**；docker-compose 的键没法 import 到
//! Rust，只能抄，抄了没有断言就是腐烂的栖息地（C2059）。
//!
//! # 期望值**派生**，不写快照
//!
//! 应用那一侧直接引用真源符号 `crate::DRAIN_LIMIT_SECS`（同 crate，能 import 就不抄）；
//! 只有 compose 那一侧要解析（`.yml` 是另一种格式）。因此把排水上限从 8 改成 15、忘了
//! 动 compose，这条门禁当场变红 —— 这正是它存在的理由。
//!
//! 设计约束（与 `deploy_gate` / `smtp_port_gate` / `body_limit_gate` 同型）：
//! **仅测试期编译**（`include_str!` 的文件不会进入发布产物）、**零新依赖**、
//! **编译期读入**（不依赖工作目录）。
//!
//! 射程（词法，如实记录）：只证「这两个**声明值**的关系」—— **不**测量真实停机窗口
//!（那要在容器里发 SIGTERM 打点，见 `docs/deployment.md` 的验收判据），也**不**校验
//! `docker-compose.yml` 里那份说明注释是不是还说得对（散文属 `doc-comment-claims` 轴：
//! 修数据、不上门禁）。

use crate::DRAIN_LIMIT_SECS;

/// 编译期读入的容器侧载体 —— 本门禁的**唯一**期望来源（应用侧的期望值来自常量本身）。
const COMPOSE: &str = include_str!("../docker-compose.yml");

/// 从 compose 里取 `stop_grace_period` 的**生效值**（秒）。
///
/// 只认未被注释的行（行首 `#` 的说明不算）；值形如 `12s` / `1m30s`，本仓用的是 `<n>s`。
/// 返回 `None` = 文件里**没有**这个键 —— 那是「声明缺席」，调用点必须把它当成违规而不是
///「跳过」（否则键被删掉也会静默通过，正是本门禁要防的那种失效）。
fn compose_stop_grace_period_secs(src: &str) -> Option<u64> {
    for line in src.lines() {
        let body = line.trim_start();
        if body.starts_with('#') {
            continue; // 注释里的示例不是生效值
        }
        let Some(rest) = body.strip_prefix("stop_grace_period") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue; // 例如 `stop_grace_period_x:` 之类，不是同一个键
        };
        let value = rest.trim().trim_matches(|c| c == '"' || c == '\'');
        let secs = value.strip_suffix('s')?;
        return secs.trim().parse::<u64>().ok();
    }
    None
}

/// 容器窗口是否真的比应用的排水上限长 —— 门禁的唯一判据。
fn grace_outlasts_drain(grace_secs: u64) -> bool {
    grace_secs > DRAIN_LIMIT_SECS
}

#[test]
fn the_compose_grace_period_outlasts_the_app_drain_limit() {
    let grace = compose_stop_grace_period_secs(COMPOSE).expect(
        "docker-compose.yml 必须显式声明 stop_grace_period：省略会退回 docker 默认 10s，\
         而应用的排水上限是另一个数，二者一旦反序就是静默的「排水被 SIGKILL 掐断」",
    );
    assert!(
        grace_outlasts_drain(grace),
        "docker 的停机窗口 stop_grace_period={grace}s 必须大于应用的排水上限 \
         DRAIN_LIMIT_SECS={DRAIN_LIMIT_SECS}s，否则应用还在排水就被 SIGKILL 打死，\
         优雅停机退化回「在途请求被掐断」且没有任何报错"
    );
}

#[test]
fn the_scanner_actually_sees_the_setting_it_guards() {
    // 阳性对照：扫描器真的读到了那行生效的键，而不是「扫到 0 条也算通过」。
    let grace =
        compose_stop_grace_period_secs(COMPOSE).expect("应读到 compose 的 stop_grace_period");
    assert!(
        grace > 0,
        "读到的 stop_grace_period 应是正秒数，实际 {grace}"
    );
    // 它必须是**生效的** compose 键（同缩进层出现 `stop_grace_period:`），不是注释里的例子。
    let effective = COMPOSE.lines().any(|l| {
        !l.trim_start().starts_with('#') && l.trim_start().starts_with("stop_grace_period:")
    });
    assert!(effective, "被读到的值应来自一行生效的 `stop_grace_period:`");
}

#[test]
fn the_rule_flags_a_grace_period_that_does_not_outlast_the_drain() {
    // 牙齿：判据本身在坏形状上真的会说「不」。
    assert!(
        !grace_outlasts_drain(DRAIN_LIMIT_SECS),
        "窗口等于应用上限不算够 —— docker 的计时器与应用同时到点，没有余量"
    );
    if DRAIN_LIMIT_SECS > 0 {
        assert!(
            !grace_outlasts_drain(DRAIN_LIMIT_SECS - 1),
            "窗口小于应用上限必须判负"
        );
    }
    assert!(
        grace_outlasts_drain(DRAIN_LIMIT_SECS + 1),
        "窗口大于应用上限应判过"
    );
}

#[test]
fn the_scanner_ignores_a_commented_out_grace_period() {
    // 提取器的边界：注释行/别的键都不得被当成生效值（否则把行注释掉就能「骗过」门禁）。
    assert_eq!(
        compose_stop_grace_period_secs("    # stop_grace_period: 99s\n"),
        None,
        "注释掉的键不是生效值"
    );
    assert_eq!(
        compose_stop_grace_period_secs("    stop_grace_period_x: 99s\n"),
        None,
        "同前缀的别的键不是同一个键"
    );
    assert_eq!(
        compose_stop_grace_period_secs("    stop_grace_period: 12s\n"),
        Some(12),
        "生效的键应被读成秒数"
    );
}
