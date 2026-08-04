use tokio::sync::broadcast;

use crate::domain::event::OrderEvent;

/// AsyncEventBus: 非同期イベントバス
///
/// `tokio::sync::broadcast` を使用し、購読者が非同期でイベントを受信できる。
/// 同期版 `EventBus` とは別に動作し、必要に応じて併用可能。
#[derive(Debug)]
pub struct AsyncEventBus {
    sender: broadcast::Sender<OrderEvent>,
}

impl AsyncEventBus {
    /// チャネル容量を指定して作成（デフォルト 256）
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// 購読用レシーバーを取得
    pub fn subscribe(&self) -> broadcast::Receiver<OrderEvent> {
        self.sender.subscribe()
    }

    /// イベントを同期的に発行（バッファリングして即時リターン）
    pub fn publish(&self, event: OrderEvent) {
        let _ = self.sender.send(event);
    }

    /// 購読者数を取得
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for AsyncEventBus {
    fn default() -> Self {
        Self::new(256)
    }
}
