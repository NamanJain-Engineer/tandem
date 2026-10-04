use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;

/// One broadcast channel per topic name.
pub struct TopicBus<T: Clone + Send + 'static> {
    capacity: usize,
    topics: Mutex<HashMap<String, broadcast::Sender<T>>>,
}

impl<T: Clone + Send + 'static> TopicBus<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            topics: Mutex::new(HashMap::new()),
        }
    }

    fn sender(&self, topic: &str) -> broadcast::Sender<T> {
        let mut topics = self.topics.lock().unwrap_or_else(|err| err.into_inner());
        topics
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.capacity).0)
            .clone()
    }

    pub fn subscribe(&self, topic: &str) -> broadcast::Receiver<T> {
        self.sender(topic).subscribe()
    }

    pub fn publish(
        &self,
        topic: &str,
        message: T,
    ) -> Result<usize, broadcast::error::SendError<T>> {
        self.sender(topic).send(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn topics_do_not_cross() {
        let bus = TopicBus::<String>::new(8);
        let mut agents = bus.subscribe("agents");
        let mut panes = bus.subscribe("panes");
        bus.publish("agents", "working".to_string())
            .expect("publish");
        assert_eq!(agents.recv().await.expect("recv"), "working");
        assert!(panes.try_recv().is_err());
    }
}
