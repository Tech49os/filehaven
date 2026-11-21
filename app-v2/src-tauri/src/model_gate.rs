//! 模型占用闸（P3 §3.4）：目录建议 / 文本深读 / 视觉深读 三向互斥。
//!
//! 背景：8GB 显存装不下两个模型同时驻留（文本 7b ≈ 4.4GB、视觉 9b ≈ 6.6GB），
//! 任何两个请求并发都会把 llama-server 打崩。P2 的 `paused_for_suggest` 是个
//! 双人锁（只管目录建议 vs 队列），P3 扩成一把显式闸：谁在跑谁占位，
//! 别人等（最多 70s，等不到就跳过这一轮，不死等）。

/// 谁占着模型。三个取值对应三条会发模型请求的通路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// 目录建议（suggest_dirs::run）
    Suggest,
    /// 深度标签 · 文本通道
    Text,
    /// 深度标签 · 视觉通道（9b 独占显存）
    Vision,
}

impl Owner {
    pub fn as_str(&self) -> &'static str {
        match self {
            Owner::Suggest => "suggest",
            Owner::Text => "text",
            Owner::Vision => "vision",
        }
    }
}

/// 一把闸。`None` = 空闲；`Some(owner)` = 被谁占着。
#[derive(Default)]
pub struct ModelGate {
    owner: std::sync::Mutex<Option<Owner>>,
}

impl ModelGate {
    /// 尝试占用，最多等 `timeout_ms`；拿不到返回 false（调用方跳过这一轮，不死等）。
    ///
    /// 等待期间每 50ms 探一次。锁只在做判定和占位的瞬间持有，不含睡眠。
    pub fn acquire(&self, who: Owner, timeout_ms: u64) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Ok(mut g) = self.owner.lock() {
                if g.is_none() {
                    *g = Some(who);
                    return true;
                }
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// 释放。只认持有者本人：别人（或重复释放）动不了它。
    pub fn release(&self, who: Owner) {
        if let Ok(mut g) = self.owner.lock() {
            if *g == Some(who) {
                *g = None;
            }
        }
    }

    /// 现在谁占着（诊断用）。
    pub fn owner(&self) -> Option<Owner> {
        self.owner.lock().ok().and_then(|g| *g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_then_release_roundtrip() {
        let g = ModelGate::default();
        assert!(g.acquire(Owner::Text, 0), "空闲时立刻拿到");
        assert_eq!(g.owner(), Some(Owner::Text));
        g.release(Owner::Text);
        assert_eq!(g.owner(), None);
        assert!(g.acquire(Owner::Vision, 0), "释放后别人能拿到");
    }

    #[test]
    fn second_owner_waits_and_times_out() {
        let g = ModelGate::default();
        assert!(g.acquire(Owner::Text, 0));
        let t0 = std::time::Instant::now();
        assert!(
            !g.acquire(Owner::Suggest, 200),
            "被占着时拿不到，必须返回 false（不死等）"
        );
        assert!(
            t0.elapsed().as_millis() >= 190,
            "不许立刻返回，那是没等"
        );
        // 超时返回后占位者不变
        assert_eq!(g.owner(), Some(Owner::Text));
    }

    #[test]
    fn waiter_gets_gate_after_holder_releases() {
        let g = std::sync::Arc::new(ModelGate::default());
        assert!(g.acquire(Owner::Text, 0));
        let g2 = g.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            g2.release(Owner::Text);
        });
        let t0 = std::time::Instant::now();
        assert!(
            g.acquire(Owner::Suggest, 3000),
            "持有者释放后，等待方要拿到"
        );
        assert!(
            t0.elapsed().as_millis() >= 250,
            "应该在释放点附近才拿到，不是立刻"
        );
        assert_eq!(g.owner(), Some(Owner::Suggest));
    }

    #[test]
    fn release_by_other_owner_is_ignored() {
        let g = ModelGate::default();
        assert!(g.acquire(Owner::Text, 0));
        g.release(Owner::Suggest); // 不是持有者，不许放
        g.release(Owner::Vision);
        assert_eq!(g.owner(), Some(Owner::Text), "别人放不掉持有者的锁");
        g.release(Owner::Text); // 本人放
        assert_eq!(g.owner(), None);
    }

    #[test]
    fn owner_names_are_stable() {
        assert_eq!(Owner::Suggest.as_str(), "suggest");
        assert_eq!(Owner::Text.as_str(), "text");
        assert_eq!(Owner::Vision.as_str(), "vision");
    }
}
