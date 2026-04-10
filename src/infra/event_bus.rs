use crate::domain::event::OrderEvent;

pub trait EventHandler: Send + Sync {
    fn handle(&self, event: &OrderEvent);
}

#[derive(Default)]
pub struct EventBus {
    handlers: Vec<Box<dyn EventHandler>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    pub fn publish(&self, event: &OrderEvent) {
        for handler in &self.handlers {
            handler.handle(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::OrderId;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHandler {
        count: Arc<AtomicUsize>,
    }

    impl EventHandler for CountingHandler {
        fn handle(&self, _event: &OrderEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn publish_to_multiple_handlers() {
        let mut bus = EventBus::new();
        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        bus.register(Box::new(CountingHandler { count: count1.clone() }));
        bus.register(Box::new(CountingHandler { count: count2.clone() }));

        let event = OrderEvent::Created { order_id: OrderId::new() };
        bus.publish(&event);

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn no_handlers_ok() {
        let bus = EventBus::new();
        let event = OrderEvent::Created { order_id: OrderId::new() };
        bus.publish(&event); // should not panic
    }
}
