use tokio::sync::broadcast::{
    self,
    error::{RecvError, SendError},
    Receiver, Sender,
};

use crate::message::Message;

#[derive(Clone, Debug)]
pub enum Event {
    NewConnection(u32),
    MessageReceived(Message, u32),
    NetworkEstablished,
    ShouldForward(Message, u32),
}

pub struct EventBus {
    sender: Sender<Event>,
}

impl EventBus {
    pub fn new() -> EventBus {
        let (sender, _) = broadcast::channel::<Event>(64);
        return EventBus { sender };
    }

    pub fn subscribe(&self) -> Receiver<Event> {
        return self.sender.subscribe();
    }

    pub fn emit(&self, event: Event) -> Result<usize, SendError<Event>> {
        return self.sender.send(event);
    }

    pub async fn wait_for<T, U: Fn(Event) -> Option<T>>(&self, func: U) -> Result<T, RecvError> {
        let mut receiver = self.subscribe();
        loop {
            match receiver.recv().await {
                Ok(ev) => match func(ev) {
                    Some(data) => return Ok(data),
                    None => {}
                },
                Err(err) => return Err(err),
            }
        }
    }
}
