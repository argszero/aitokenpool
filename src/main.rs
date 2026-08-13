//! AITokenPool — AI Token 共享池
//!
//! 企业版：内部 key 池 + 员工点数配额
//! 公共版：分享闲置 key 赚点数、消费别人 key
//!
//! 架构定论见 docs/architecture.md（中心化方案 A：平台托管 key + 平台执行调用）

fn main() {
    println!("AITokenPool — AI Token 共享池");
    println!("企业版：key 池 + 员工点数配额 · 公共版：共享市场");
    println!("架构：中心化（方案 A），Rust + axum");
    println!("状态：项目初始化中（2026-08-13）");
}
