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
use std::time::Duration;

pub struct BTCWebsocketActor(
    pub SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
);

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientCommand(pub String);

impl Actor for BTCWebsocketActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("Bitcoin websocket disconnected");
        // Stop application on disconnect
        System::current().stop();
    }
}

impl BTCWebsocketActor {
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            act.0.write(Message::Ping(Bytes::from_static(b""))).unwrap();
            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }
}

/// Handle stdin commands
impl Handler<ClientCommand> for BTCWebsocketActor {
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, _ctx: &mut Context<Self>) {
        self.0.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for BTCWebsocketActor {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        if let Ok(Frame::Text(txt)) = msg {
            println!("Server: {:?}", txt)
        }
    }

    fn started(&mut self, ctx: &mut Context<Self>) {
        println!("Bitcoin websocket connected");
    
        let mut _cmd = String::from("{\"op\":\"ping\"}");
        let cmd = String::from("{\"op\":\"unconfirmed_sub\"}");
        
        ctx.address().do_send(ClientCommand(cmd));
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for BTCWebsocketActor {}
