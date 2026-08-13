//! AITokenPool — AI Token 共享池
//!
//! 企业版：内部 key 池 + 员工点数配额
//! 公共版：分享闲置 key 赚点数、消费别人 key
//!
//! 架构定论见 docs/architecture.md（中心化方案 A：平台托管 key + 平台执行调用）

/// 返回项目横幅文本（便于测试与展示）
pub fn banner() -> String {
    "AITokenPool — AI Token 共享池\n\
     企业版：key 池 + 员工点数配额 · 公共版：共享市场\n\
     架构：中心化（方案 A），Rust + axum\n\
     状态：项目初始化中（2026-08-13）"
        .to_string()
}

fn main() {
    println!("{}", banner());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_contains_project_identity() {
        let b = banner();
        assert!(b.contains("AITokenPool"));
        assert!(b.contains("企业版"));
        assert!(b.contains("公共版"));
        assert!(b.contains("中心化"));
    }
}
