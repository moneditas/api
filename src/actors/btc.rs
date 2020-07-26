use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
    BoxedSocket,
};
use bytes::Bytes;
use futures::stream::SplitSink;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

// How often heartbeat pings are sent
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Transaction(pub Bytes);

#[derive(Message)]
#[rtype(result = "String")]
pub struct Subscribe(pub Recipient<Transaction>);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Unsubscribe(pub String);

pub struct BitcoinActor {
    pub sink: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
    subscribers: HashMap<String, Recipient<Transaction>>,
    heartbeat_at: Instant,
}

impl Actor for BitcoinActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("BitcoinActor websocket disconnected");
        // Stop application on disconnect
        System::current().stop();
    }
}

impl BitcoinActor {
    pub fn new(sink: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>) -> Self {
        Self {
            sink,
            subscribers: HashMap::new(),
            heartbeat_at: Instant::now(),
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(CLIENT_HEARTBEAT_INTERVAL, |act, _ctx| {
            if Instant::now().duration_since(act.heartbeat_at) > CLIENT_TIMEOUT {
                println!("Websocket client heartbeat failed!");

                // TODO: Implement a reconnection logic besides subscribing again
                act.sink
                    .write(Message::Text(String::from("{\"op\":\"unconfirmed_sub\"}")))
                    .unwrap();
            }

            act.sink
                .write(Message::Text(String::from("{\"op\":\"ping\"}")))
                .unwrap();
        });
    }

    fn notify(&mut self, transaction: Bytes) {
        self.subscribers.iter().for_each(|(id, addr)| {
            let is_err = addr.do_send(Transaction(transaction.clone())).is_err();
            if is_err {
                println!("Failed to send message to subscriber {}", id)
            }
        });
    }
}

impl Handler<Subscribe> for BitcoinActor {
    type Result = String;

    fn handle(&mut self, msg: Subscribe, _: &mut Self::Context) -> Self::Result {
        let id = Uuid::new_v4().to_string();
        self.subscribers.insert(id.clone(), msg.0);

        println!("New client subscribed {}", &id);
        id
    }
}

impl Handler<Unsubscribe> for BitcoinActor {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _: &mut Context<Self>) {
        println!("Unsubscribing client {}", &msg.0);
        self.subscribers.remove(&msg.0);
    }
}

impl StreamHandler<Result<Frame, WsProtocolError>> for BitcoinActor {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        self.heartbeat_at = Instant::now();

        if let Ok(Frame::Text(txt)) = msg {
            self.notify(txt);
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        println!("BitcoinActor websocket connected");

        self.sink
            .write(Message::Text(String::from("{\"op\":\"unconfirmed_sub\"}")))
            .unwrap();
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for BitcoinActor {}
