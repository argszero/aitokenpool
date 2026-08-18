//! 路由与故障转移（architecture §4.2.1 七条定案）
//!
//! P0-B（rant 2026-08-18T09:55:57）：
//! - 初始随机：从模型 M 的健康 key（status='on' 且不在冷却期）中随机选一
//! - 粘性：用户+模型 → 复用上次 key（进程内 HashMap<(user_id, model), key_id>）
//! - 不可用判定：上游 401/403/429/5xx 或网络错误 → 标记非健康 5 秒（冷却），静默切换
//! - 切换上限 3 次：3 次全失败 → 503「该模型暂无可用 key」（由网关层返回）

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;

use crate::dao::KeyRow;

/// 默认冷却时长（architecture §4.2.1 定案：5 秒）
pub const COOLDOWN_SECS: u64 = 5;
/// 每次路由最多尝试的 key 数（初始 + 2 次切换）
pub const MAX_SWITCHES: usize = 3;

/// 路由状态（进程内）：粘性映射 + 冷却表
#[derive(Default)]
pub struct RouterState {
    sticky: Mutex<HashMap<(i64, String), i64>>,
    cooldown: Mutex<HashMap<i64, Instant>>,
    cooldown_duration: Duration,
}

impl RouterState {
    pub fn new() -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
            cooldown: Mutex::new(HashMap::new()),
            cooldown_duration: Duration::from_secs(COOLDOWN_SECS),
        }
    }

    /// 测试用：自定义冷却时长
    #[cfg(test)]
    pub fn with_cooldown(d: Duration) -> Self {
        Self {
            sticky: Mutex::new(HashMap::new()),
            cooldown: Mutex::new(HashMap::new()),
            cooldown_duration: d,
        }
    }

    /// key 当前是否健康（不在冷却期内）
    pub fn is_healthy(&self, key_id: i64) -> bool {
        let c = self.cooldown.lock().expect("cooldown lock");
        match c.get(&key_id) {
            None => true,
            Some(until) => Instant::now() >= *until,
        }
    }

    /// 标记 key 非健康（进入冷却），并清除该 key 的所有粘性引用
    pub fn mark_unhealthy(&self, key_id: i64) {
        self.cooldown
            .lock()
            .expect("cooldown lock")
            .insert(key_id, Instant::now() + self.cooldown_duration);
        self.sticky
            .lock()
            .expect("sticky lock")
            .retain(|_, v| *v != key_id);
    }

    /// 记录粘性：该用户+模型后续复用此 key
    pub fn mark_sticky(&self, user_id: i64, model: &str, key_id: i64) {
        self.sticky
            .lock()
            .expect("sticky lock")
            .insert((user_id, model.to_string()), key_id);
    }

    /// 选 key：粘性优先（仍健康），否则从健康集合随机
    /// 无健康 key → None
    pub fn pick(&self, keys: &[KeyRow], user_id: i64, model: &str) -> Option<i64> {
        let healthy: Vec<&KeyRow> = keys.iter().filter(|k| self.is_healthy(k.id)).collect();
        if healthy.is_empty() {
            return None;
        }
        {
            let s = self.sticky.lock().expect("sticky lock");
            if let Some(kid) = s.get(&(user_id, model.to_string())) {
                if healthy.iter().any(|k| k.id == *kid) {
                    return Some(*kid);
                }
            }
        }
        let mut rng = rand::thread_rng();
        healthy.choose(&mut rng).map(|k| k.id)
    }

    /// 当前冷却中的 key 数（测试/诊断用）
    #[cfg(test)]
    pub fn cooldown_len(&self) -> usize {
        self.cooldown.lock().expect("cooldown lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ids: &[i64]) -> Vec<KeyRow> {
        ids.iter()
            .map(|id| KeyRow {
                id: *id,
                provider: "test".into(),
                plan: "test-plan".into(),
                owner_id: 1,
                encrypted_key: "sk-test".into(),
            })
            .collect()
    }

    #[test]
    fn pick_prefers_sticky_key() {
        let r = RouterState::new();
        let ks = keys(&[1, 2]);
        // 标记粘性 → 两次选择都回到 sticky key
        r.mark_sticky(7, "m", 2);
        let p1 = r.pick(&ks, 7, "m").unwrap();
        let p2 = r.pick(&ks, 7, "m").unwrap();
        assert_eq!(p1, 2);
        assert_eq!(p2, 2);
    }

    #[test]
    fn random_without_sticky_is_from_healthy_set() {
        let r = RouterState::new();
        let ks = keys(&[1, 2, 3]);
        for _ in 0..20 {
            let p = r.pick(&ks, 7, "m").unwrap();
            assert!(ks.iter().any(|k| k.id == p));
        }
    }

    #[test]
    fn cooldown_excludes_and_expires() {
        let r = RouterState::with_cooldown(Duration::from_millis(50));
        let ks = keys(&[1, 2]);
        assert!(r.is_healthy(1));
        r.mark_unhealthy(1);
        assert!(!r.is_healthy(1));
        // 冷却中的 key 不会被选中
        for _ in 0..20 {
            assert_eq!(r.pick(&ks, 7, "m").unwrap(), 2);
        }
        assert_eq!(r.cooldown_len(), 1);
        // 冷却过期后恢复健康
        std::thread::sleep(Duration::from_millis(70));
        assert!(r.is_healthy(1));
        assert_eq!(r.cooldown_len(), 1, "过期条目保留但视为健康");
    }

    #[test]
    fn mark_unhealthy_clears_sticky() {
        let r = RouterState::new();
        let ks = keys(&[1, 2]);
        r.mark_sticky(7, "m", 1);
        r.mark_unhealthy(1);
        // sticky key 不可用 → 只能选 2
        assert_eq!(r.pick(&ks, 7, "m").unwrap(), 2);
    }

    #[test]
    fn no_healthy_key_returns_none() {
        let r = RouterState::new();
        let ks = keys(&[1]);
        r.mark_unhealthy(1);
        assert!(r.pick(&ks, 7, "m").is_none());
    }

    #[test]
    fn default_cooldown_is_5_secs() {
        let r = RouterState::new();
        assert_eq!(r.cooldown_duration, Duration::from_secs(COOLDOWN_SECS));
    }
}
