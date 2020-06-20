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
use std::time::Duration;
use uuid::Uuid;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientCommand(pub String);

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
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.sink
                .write(Message::Ping(Bytes::from_static(b"")))
                .unwrap();

            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }

    fn notify(&mut self, transaction: Bytes) {
        self.subscribers.iter().for_each(|(id, addr)| {
            let is_err = addr.do_send(Transaction(transaction.clone())).is_err();
            if is_err {
                println!("Failed sending message to subscriber {}", id)
            }
        });
    }
}

/// Subscribe to transaction event
impl Handler<Subscribe> for BitcoinActor {
    type Result = String;

    fn handle(&mut self, msg: Subscribe, _: &mut Self::Context) -> Self::Result {
        let id = Uuid::new_v4().to_string();
        self.subscribers.insert(id.clone(), msg.0);

        println!("New client subscribed {}", &id);
        id
    }
}

/// Handler for Unsubscribe message.
impl Handler<Unsubscribe> for BitcoinActor {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _: &mut Context<Self>) {
        println!("Unsubscribing client {}", &msg.0);
        self.subscribers.remove(&msg.0);
    }
}

/// Handle stdin commands
impl Handler<ClientCommand> for BitcoinActor {
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, _ctx: &mut Context<Self>) {
        self.sink.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for BitcoinActor {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        if let Ok(Frame::Text(txt)) = msg {
            self.notify(txt);
        }
    }

    fn started(&mut self, ctx: &mut Context<Self>) {
        println!("BitcoinActor websocket connected");

        let mut _cmd = String::from("{\"op\":\"ping\"}");
        let cmd = String::from("{\"op\":\"unconfirmed_sub\"}");

        ctx.address().do_send(ClientCommand(cmd));
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for BitcoinActor {}
