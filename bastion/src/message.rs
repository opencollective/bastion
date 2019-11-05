use crate::children::Children;
use crate::context::BastionId;
use crate::supervisor::{SupervisionStrategy, Supervisor};
use futures::channel::oneshot::{self, Receiver};
use std::any::Any;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub trait Message: Any + Send + Sync + Debug {}
impl<T> Message for T where T: Any + Send + Sync + Debug {}

#[derive(Debug)]
#[doc(hidden)]
pub struct Sender(oneshot::Sender<Msg>);

#[derive(Debug)]
pub struct Answer(Receiver<Msg>);

#[derive(Debug)]
pub struct Msg(MsgInner);

#[derive(Debug)]
enum MsgInner {
    Broadcast(Arc<dyn Any + Send + Sync + 'static>),
    Tell(Box<dyn Any + Send + Sync + 'static>),
    Ask {
        msg: Box<dyn Any + Send + Sync + 'static>,
        sender: Option<Sender>,
    },
}

#[derive(Debug)]
pub(crate) enum BastionMessage {
    Start,
    Stop,
    Kill,
    Deploy(Deployment),
    Prune { id: BastionId },
    SuperviseWith(SupervisionStrategy),
    Message(Msg),
    Stopped { id: BastionId },
    Faulted { id: BastionId },
}

#[derive(Debug)]
pub(crate) enum Deployment {
    Supervisor(Supervisor),
    Children(Children),
}

impl Sender {
    #[doc(hidden)]
    pub fn send<M: Message>(self, msg: M) -> Result<(), M> {
        let msg = Msg::tell(msg);
        self.0.send(msg).map_err(|msg| msg.try_unwrap().unwrap())
    }
}

impl Msg {
    pub(crate) fn broadcast<M: Message>(msg: M) -> Self {
        let inner = MsgInner::Broadcast(Arc::new(msg));
        Msg(inner)
    }

    pub(crate) fn tell<M: Message>(msg: M) -> Self {
        let inner = MsgInner::Tell(Box::new(msg));
        Msg(inner)
    }

    pub(crate) fn ask<M: Message>(msg: M) -> (Self, Answer) {
        let msg = Box::new(msg);
        let (sender, recver) = oneshot::channel();
        let sender = Sender(sender);
        let answer = Answer(recver);

        let sender = Some(sender);
        let inner = MsgInner::Ask { msg, sender };

        (Msg(inner), answer)
    }

    #[doc(hidden)]
    pub fn is_broadcast(&self) -> bool {
        if let MsgInner::Broadcast(_) = self.0 {
            true
        } else {
            false
        }
    }

    #[doc(hidden)]
    pub fn is_tell(&self) -> bool {
        if let MsgInner::Tell(_) = self.0 {
            true
        } else {
            false
        }
    }

    #[doc(hidden)]
    pub fn is_ask(&self) -> bool {
        if let MsgInner::Ask { .. } = self.0 {
            true
        } else {
            false
        }
    }

    #[doc(hidden)]
    pub fn take_sender(&mut self) -> Option<Sender> {
        if let MsgInner::Ask { sender, .. } = &mut self.0 {
            sender.take()
        } else {
            None
        }
    }

    #[doc(hidden)]
    pub fn downcast<M: Message>(self) -> Result<M, Self> {
        match self.0 {
            MsgInner::Tell(msg) => {
                if msg.is::<M>() {
                    let msg: Box<dyn Any + 'static> = msg;
                    Ok(*msg.downcast().unwrap())
                } else {
                    let inner = MsgInner::Tell(msg);
                    Err(Msg(inner))
                }
            }
            MsgInner::Ask { msg, sender } => {
                if msg.is::<M>() {
                    let msg: Box<dyn Any + 'static> = msg;
                    Ok(*msg.downcast().unwrap())
                } else {
                    let inner = MsgInner::Ask { msg, sender };
                    Err(Msg(inner))
                }
            }
            _ => Err(self),
        }
    }

    #[doc(hidden)]
    pub fn downcast_ref<M: Message>(&self) -> Option<Arc<M>> {
        if let MsgInner::Broadcast(msg) = &self.0 {
            if msg.is::<M>() {
                return Some(msg.clone().downcast::<M>().unwrap());
            }
        }

        None
    }

    pub(crate) fn try_clone(&self) -> Option<Self> {
        if let MsgInner::Broadcast(msg) = &self.0 {
            let inner = MsgInner::Broadcast(msg.clone());
            Some(Msg(inner))
        } else {
            None
        }
    }

    pub(crate) fn try_unwrap<M: Message>(self) -> Result<M, Self> {
        if let MsgInner::Broadcast(msg) = self.0 {
            match msg.downcast() {
                Ok(msg) => match Arc::try_unwrap(msg) {
                    Ok(msg) => Ok(msg),
                    Err(msg) => {
                        let inner = MsgInner::Broadcast(msg);
                        Err(Msg(inner))
                    }
                },
                Err(msg) => {
                    let inner = MsgInner::Broadcast(msg);
                    Err(Msg(inner))
                }
            }
        } else {
            self.downcast()
        }
    }
}

impl BastionMessage {
    pub(crate) fn start() -> Self {
        BastionMessage::Start
    }

    pub(crate) fn stop() -> Self {
        BastionMessage::Stop
    }

    pub(crate) fn kill() -> Self {
        BastionMessage::Kill
    }

    pub(crate) fn deploy_supervisor(supervisor: Supervisor) -> Self {
        let deployment = Deployment::Supervisor(supervisor);

        BastionMessage::Deploy(deployment)
    }

    pub(crate) fn deploy_children(children: Children) -> Self {
        let deployment = Deployment::Children(children);

        BastionMessage::Deploy(deployment)
    }

    pub(crate) fn prune(id: BastionId) -> Self {
        BastionMessage::Prune { id }
    }

    pub(crate) fn supervise_with(strategy: SupervisionStrategy) -> Self {
        BastionMessage::SuperviseWith(strategy)
    }

    pub(crate) fn broadcast<M: Message>(msg: M) -> Self {
        let msg = Msg::broadcast(msg);
        BastionMessage::Message(msg)
    }

    pub(crate) fn tell<M: Message>(msg: M) -> Self {
        let msg = Msg::tell(msg);
        BastionMessage::Message(msg)
    }

    pub(crate) fn ask<M: Message>(msg: M) -> (Self, Answer) {
        let (msg, answer) = Msg::ask(msg);
        (BastionMessage::Message(msg), answer)
    }

    pub(crate) fn stopped(id: BastionId) -> Self {
        BastionMessage::Stopped { id }
    }

    pub(crate) fn faulted(id: BastionId) -> Self {
        BastionMessage::Faulted { id }
    }

    pub(crate) fn try_clone(&self) -> Option<Self> {
        let clone = match self {
            BastionMessage::Start => BastionMessage::start(),
            BastionMessage::Stop => BastionMessage::stop(),
            BastionMessage::Kill => BastionMessage::kill(),
            // FIXME
            BastionMessage::Deploy(_) => unimplemented!(),
            BastionMessage::Prune { id } => BastionMessage::prune(id.clone()),
            BastionMessage::SuperviseWith(strategy) => {
                BastionMessage::supervise_with(strategy.clone())
            }
            BastionMessage::Message(msg) => BastionMessage::Message(msg.try_clone()?),
            BastionMessage::Stopped { id } => BastionMessage::stopped(id.clone()),
            BastionMessage::Faulted { id } => BastionMessage::faulted(id.clone()),
        };

        Some(clone)
    }

    pub(crate) fn into_msg<M: Message>(self) -> Option<M> {
        if let BastionMessage::Message(msg) = self {
            msg.try_unwrap().ok()
        } else {
            None
        }
    }
}

impl Future for Answer {
    type Output = Result<Msg, ()>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(ctx).map_err(|_| ())
    }
}

#[macro_export]
/// Matches a [`Msg`] (as returned by [`BastionContext::recv`]
/// or [`BastionContext::try_recv`]) with different types.
///
/// Each case is defined as:
/// - an optional `ref` which will make the case only match
///   if the message was broadcasted
/// - a variable name for the message if it matched this case
/// - a colon
/// - a type that the message must be of to match this case
///   (note that if the message was broadcasted, the actual
///   type of the variable will be a reference to this type)
/// - an arrow (`=>`) with an optional bang (`!`) between
///   the equal and greater-than signs which will make the
///   case only match if the message can be answered
/// - code that will be executed if the case matches
///
/// If the message can be answered (when using `=!>` instead
/// of `=>` as said above), an answer can be sent by passing
/// it to the `answer!` macro that will be generated for this
/// use.
///
/// A default case is required, which is defined in the same
/// way as any other case but with its type set as `_` (note
/// that it doesn't has the optional `ref` or `=!>`).
///
/// # Example
///
/// ```
/// # use bastion::prelude::*;
/// #
/// # fn main() {
///     # Bastion::init();
/// // The message that will be broadcasted...
/// const BCAST_MSG: &'static str = "A message containing data (broadcast).";
/// // The message that will be "told" to the child...
/// const TELL_MSG: &'static str = "A message containing data (tell).";
/// // The message that will be "asked" to the child...
/// const ASK_MSG: &'static str = "A message containing data (ask).";
///
/// Bastion::children(|children| {
///     children.with_exec(|ctx: BastionContext| {
///         async move {
///             # ctx.current().tell(TELL_MSG).unwrap();
///             # ctx.current().ask(ASK_MSG).unwrap();
///             #
///             loop {
///                 msg! { ctx.recv().await?,
///                     // We match broadcasted `&'static str`s...
///                     ref msg: &'static str => {
///                         // Note that `msg` will actually be a `&&'static str`.
///                         assert_eq!(msg, &BCAST_MSG);
///                         // Handle the message...
///                     };
///                     // We match `&'static str`s "told" to this child...
///                     msg: &'static str => {
///                         assert_eq!(msg, TELL_MSG);
///                         // Handle the message...
///                     };
///                     // We match `&'static str`'s "asked" to this child...
///                     msg: &'static str =!> {
///                         assert_eq!(msg, ASK_MSG);
///                         // Handle the message...
///
///                         // ...and eventually answer to it...
///                         answer!("An answer to the message.");
///                     };
///                     // We are only broadcasting, "telling" and "asking" a
///                     // `&'static str` in this example, so we know that this won't
///                     // happen...
///                     _: _ => ();
///                 }
///             }
///         }
///     })
/// }).expect("Couldn't start the children group.");
///     #
///     # Bastion::start();
///     # Bastion::broadcast(BCAST_MSG).unwrap();
///     # Bastion::stop();
///     # Bastion::block_until_stopped();
/// # }
/// ```
///
/// [`Msg`]: children/struct.Msg.html
/// [`BastionContext::recv`]: struct.BastionContext.html#method.recv
/// [`BastionContext::try_recv`]: struct.BastionContext.html#method.try_recv
macro_rules! msg {
    ($msg:expr, $($tokens:tt)+) => {
        { msg!(@internal $msg, (), (), (), $($tokens)+); }
    };

    (@internal
        $msg:expr,
        ($($bvar:ident, $bty:ty, $bhandle:expr,)*),
        ($($tvar:ident, $tty:ty, $thandle:expr,)*),
        ($($avar:ident, $aty:ty, $ahandle:expr,)*),
        ref $var:ident: $ty:ty => $handle:expr;
        $($rest:tt)+
    ) => {
        msg!(@internal $msg,
            ($($bvar, $bty, $bhandle,)* $var, $ty, $handle,),
            ($($tvar, $tty, $thandle,)*),
            ($($avar, $aty, $ahandle,)*),
            $($rest)+
        )
    };

    (@internal
        $msg:expr,
        ($($bvar:ident, $bty:ty, $bhandle:expr,)*),
        ($($tvar:ident, $tty:ty, $thandle:expr,)*),
        ($($avar:ident, $aty:ty, $ahandle:expr,)*),
        $var:ident: $ty:ty => $handle:expr;
        $($rest:tt)+
    ) => {
        msg!(@internal $msg,
            ($($bvar, $bty, $bhandle,)*),
            ($($tvar, $tty, $thandle,)* $var, $ty, $handle,),
            ($($avar, $aty, $ahandle,)*),
            $($rest)+
        )
    };

    (@internal
        $msg:expr,
        ($($bvar:ident, $bty:ty, $bhandle:expr,)*),
        ($($tvar:ident, $tty:ty, $thandle:expr,)*),
        ($($avar:ident, $aty:ty, $ahandle:expr,)*),
        $var:ident: $ty:ty =!> $handle:expr;
        $($rest:tt)+
    ) => {
        msg!(@internal $msg,
            ($($bvar, $bty, $bhandle,)*),
            ($($tvar, $tty, $thandle,)*),
            ($($avar, $aty, $ahandle,)* $var, $ty, $handle,),
            $($rest)+
        )
    };

    (@internal
        $msg:expr,
        ($($bvar:ident, $bty:ty, $bhandle:expr,)*),
        ($($tvar:ident, $tty:ty, $thandle:expr,)*),
        ($($avar:ident, $aty:ty, $ahandle:expr,)*),
        _: _ => $handle:expr;
    ) => {
        msg!(@internal $msg,
            ($($bvar, $bty, $bhandle,)*),
            ($($tvar, $tty, $thandle,)*),
            ($($avar, $aty, $ahandle,)*),
            msg: _ => $handle;
        )
    };

    (@internal
        $msg:expr,
        ($($bvar:ident, $bty:ty, $bhandle:expr,)*),
        ($($tvar:ident, $tty:ty, $thandle:expr,)*),
        ($($avar:ident, $aty:ty, $ahandle:expr,)*),
        $var:ident: _ => $handle:expr;
    ) => {
        let mut $var = $msg;
        let sender = $var.take_sender();
        if $var.is_broadcast() {
            if false {}
            $(
                else if let Some($bvar) = $var.downcast_ref::<$bty>() {
                    let $bvar = &*$bvar;
                    { $bhandle };
                }
            )*
            else {
                { $handle };
            }
        } else if sender.is_some() {
            let sender = sender.unwrap();
            macro_rules! answer {
                ($answer:expr) => {
                    sender.send($answer)
                };
            }

            loop {
                $(
                    match $var.downcast::<$aty>() {
                        Ok($avar) => {
                            { $ahandle };
                            break;
                        }
                        Err(msg_) => $var = msg_,
                    }
                )*

                { $handle };
                break;
            }
        } else {
            loop {
                $(
                    match $var.downcast::<$tty>() {
                        Ok($tvar) => {
                            { $thandle };
                            break;
                        }
                        Err(msg_) => $var = msg_,
                    }
                )*

                { $handle };
                break;
            }
        }
    };
}
