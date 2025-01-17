//!
//! A context allows a child's future to access its received
//! messages, parent and supervisor.

use crate::children::{ChildRef, ChildrenRef};
use crate::message::Msg;
use crate::supervisor::SupervisorRef;
use futures::pending;
use qutex::{Guard, Qutex};
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

pub(crate) const NIL_ID: BastionId = BastionId(Uuid::nil());

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
/// An identifier used by supervisors, children groups and
/// their elements to identify themselves, using a v4 UUID.
///
/// A `BastionId` is unique to its attached element and is
/// reset when it is restarted. A special `BastionId` exists
/// for the "system supervisor" (the supervisor created by
/// the system at startup) which is a nil UUID
/// (00000000-0000-0000-0000-000000000000).
///
/// # Example
///
/// ```rust
/// # use bastion::prelude::*;
/// #
/// # fn main() {
///     # Bastion::init();
///     #
/// Bastion::children(|children| {
///     children.with_exec(|ctx| {
///         async move {
///             let child_id: &BastionId = ctx.current().id();
///             // ...
///             # Ok(())
///         }
///     })
/// }).expect("Couldn't create the children group.");
///     #
///     # Bastion::start();
///     # Bastion::stop();
///     # Bastion::block_until_stopped();
/// # }
/// ```
pub struct BastionId(Uuid);

#[derive(Debug)]
/// A child's execution context, allowing its [`exec`] future
/// to receive messages and access a [`ChildRef`] referencing
/// it, a [`ChildrenRef`] referencing its children group and
/// a [`SupervisorRef`] referencing its supervisor.
///
/// # Example
///
/// ```rust
/// # use bastion::prelude::*;
/// #
/// # fn main() {
///     # Bastion::init();
///     #
/// Bastion::children(|children| {
///     children.with_exec(|ctx: BastionContext| {
///         async move {
///             // Get a `ChildRef` referencing the child executing
///             // this future...
///             let current: &ChildRef = ctx.current();
///             // Get a `ChildrenRef` referencing the children
///             // group of the child executing this future...
///             let parent: &ChildrenRef = ctx.parent();
///             // Try to get a `SupervisorRef` referencing the
///             // supervisor of the child executing this future...
///             let supervisor: Option<&SupervisorRef> = ctx.supervisor();
///             // Note that `supervisor` will be `None` because
///             // this child was created using `Bastion::children`,
///             // which made it supervised by the system supervisor
///             // (which users can't get a reference to).
///
///             // Try to receive a message...
///             let opt_msg: Option<Msg> = ctx.try_recv().await;
///             // Wait for a message to be received...
///             let msg: Msg = ctx.recv().await?;
///
///             Ok(())
///         }
///     })
/// }).expect("Couldn't create the children group.");
///     #
///     # Bastion::start();
///     # Bastion::stop();
///     # Bastion::block_until_stopped();
/// # }
/// ```
pub struct BastionContext {
    id: BastionId,
    child: ChildRef,
    children: ChildrenRef,
    supervisor: Option<SupervisorRef>,
    state: Qutex<ContextState>,
}

#[derive(Debug)]
pub(crate) struct ContextState {
    msgs: VecDeque<Msg>,
}

impl BastionId {
    pub(crate) fn new() -> Self {
        let uuid = Uuid::new_v4();

        BastionId(uuid)
    }
}

impl BastionContext {
    pub(crate) fn new(
        id: BastionId,
        child: ChildRef,
        children: ChildrenRef,
        supervisor: Option<SupervisorRef>,
        state: Qutex<ContextState>,
    ) -> Self {
        debug!("BastionContext({}): Creating.", id);
        BastionContext {
            id,
            child,
            children,
            supervisor,
            state,
        }
    }

    /// Returns a [`ChildRef`] referencing the children group's
    /// element that is linked to this `BastionContext`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use bastion::prelude::*;
    /// #
    /// # fn main() {
    ///     # Bastion::init();
    ///     #
    /// Bastion::children(|children| {
    ///     children.with_exec(|ctx: BastionContext| {
    ///         async move {
    ///             let current: &ChildRef = ctx.current();
    ///             // Stop or kill the current element (note that this will
    ///             // only take effect after this future becomes "pending")...
    ///
    ///             Ok(())
    ///         }
    ///     })
    /// }).expect("Couldn't create the children group.");
    ///     #
    ///     # Bastion::start();
    ///     # Bastion::stop();
    ///     # Bastion::block_until_stopped();
    /// # }
    /// ```
    ///
    /// [`ChildRef`]: children/struct.ChildRef.html
    pub fn current(&self) -> &ChildRef {
        &self.child
    }

    /// Returns a [`ChildrenRef`] referencing the children group
    /// of the element that is linked to this `BastionContext`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use bastion::prelude::*;
    /// #
    /// # fn main() {
    ///     # Bastion::init();
    ///     #
    /// Bastion::children(|children| {
    ///     children.with_exec(|ctx: BastionContext| {
    ///         async move {
    ///             let parent: &ChildrenRef = ctx.parent();
    ///             // Get the other elements of the group, broadcast message,
    ///             // or stop or kill the children group...
    ///
    ///             Ok(())
    ///         }
    ///     })
    /// }).expect("Couldn't create the children group.");
    ///     #
    ///     # Bastion::start();
    ///     # Bastion::stop();
    ///     # Bastion::block_until_stopped();
    /// # }
    /// ```
    ///
    /// [`ChildrenRef`]: children/struct.ChildrenRef.html
    pub fn parent(&self) -> &ChildrenRef {
        &self.children
    }

    /// Returns a [`SupervisorRef`] referencing the supervisor
    /// that supervises the element that is linked to this
    /// `BastionContext` if it isn't the system supervisor
    /// (ie. if the children group wasn't created using
    /// [`Bastion::children`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use bastion::prelude::*;
    /// #
    /// # fn main() {
    ///     # Bastion::init();
    ///     #
    /// // When calling the method from a children group supervised
    /// // by a supervisor created by the user...
    /// Bastion::supervisor(|sp| {
    ///     sp.children(|children| {
    ///         children.with_exec(|ctx: BastionContext| {
    ///             async move {
    ///                 // ...the method will return a SupervisorRef referencing the
    ///                 // user-created supervisor...
    ///                 let supervisor: Option<&SupervisorRef> = ctx.supervisor(); // Some
    ///                 assert!(supervisor.is_some());
    ///
    ///                 Ok(())
    ///             }
    ///         })
    ///     })
    /// }).expect("Couldn't create the supervisor.");
    ///
    /// // When calling the method from a children group supervised
    /// // by the system's supervisor...
    /// Bastion::children(|children| {
    ///     children.with_exec(|ctx: BastionContext| {
    ///         async move {
    ///             // ...the method won't return a SupervisorRef...
    ///             let supervisor: Option<&SupervisorRef> = ctx.supervisor(); // None
    ///             assert!(supervisor.is_none());
    ///
    ///             Ok(())
    ///         }
    ///     })
    /// }).expect("Couldn't create the children group.");
    ///     #
    ///     # Bastion::start();
    ///     # Bastion::stop();
    ///     # Bastion::block_until_stopped();
    /// # }
    /// ```
    ///
    /// [`SupervisorRef`]: supervisor/struct.SupervisorRef.html
    /// [`Bastion::children`]: struct.Bastion.html#method.children
    pub fn supervisor(&self) -> Option<&SupervisorRef> {
        self.supervisor.as_ref()
    }

    /// Tries to retrieve asynchronously a message received by
    /// the element this `BastionContext` is linked to.
    ///
    /// If you need to wait (always asynchronously) until at
    /// least one message can be retrieved, use [`recv`] instead.
    ///
    /// This method returns [`Msg`] if a message was available, or
    /// `None otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use bastion::prelude::*;
    /// #
    /// # fn main() {
    ///     # Bastion::init();
    ///     #
    /// Bastion::children(|children| {
    ///     children.with_exec(|ctx: BastionContext| {
    ///         async move {
    ///             let opt_msg: Option<Msg> = ctx.try_recv().await;
    ///             // If a message was received by the element, `opt_msg` will
    ///             // be `Some(Msg)`, otherwise it will be `None`.
    ///
    ///             Ok(())
    ///         }
    ///     })
    /// }).expect("Couldn't create the children group.");
    ///     #
    ///     # Bastion::start();
    ///     # Bastion::stop();
    ///     # Bastion::block_until_stopped();
    /// # }
    /// ```
    ///
    /// [`recv`]: #method.recv
    /// [`Msg`]: children/struct.Msg.html
    pub async fn try_recv(&self) -> Option<Msg> {
        debug!("BastionContext({}): Trying to receive message.", self.id);
        // TODO: Err(Error)
        let mut state = self.state.clone().lock_async().await.ok()?;

        if let Some(msg) = state.msgs.pop_front() {
            trace!("BastionContext({}): Received message: {:?}", self.id, msg);
            Some(msg)
        } else {
            trace!("BastionContext({}): Received no message.", self.id);
            None
        }
    }

    /// Retrieves asynchronously a message received by the element
    /// this `BastionContext` is linked to and waits (always
    /// asynchronously) for one if none has been received yet.
    ///
    /// If you don't need to wait until at least one message
    /// can be retrieved, use [`try_recv`] instead.
    ///
    /// This method returns [`Msg`] if it succeeded, or `Err(())`
    /// otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use bastion::prelude::*;
    /// #
    /// # fn main() {
    ///     # Bastion::init();
    ///     #
    /// Bastion::children(|children| {
    ///     children.with_exec(|ctx: BastionContext| {
    ///         async move {
    ///             // This will block until a message has been received...
    ///             let msg: Msg = ctx.recv().await?;
    ///
    ///             Ok(())
    ///         }
    ///     })
    /// }).expect("Couldn't create the children group.");
    ///     #
    ///     # Bastion::start();
    ///     # Bastion::stop();
    ///     # Bastion::block_until_stopped();
    /// # }
    /// ```
    ///
    /// [`try_recv`]: #method.try_recv
    /// [`Msg`]: children/struct.Msg.html
    pub async fn recv(&self) -> Result<Msg, ()> {
        debug!("BastionContext({}): Waiting to receive message.", self.id);
        loop {
            // TODO: Err(Error)
            let mut state = self.state.clone().lock_async().await.unwrap();

            if let Some(msg) = state.msgs.pop_front() {
                trace!("BastionContext({}): Received message: {:?}", self.id, msg);
                return Ok(msg);
            }

            Guard::unlock(state);

            pending!();
        }
    }
}

impl ContextState {
    pub(crate) fn new() -> Self {
        let msgs = VecDeque::new();

        ContextState { msgs }
    }

    pub(crate) fn push_msg(&mut self, msg: Msg) {
        self.msgs.push_back(msg)
    }
}

impl Display for BastionId {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}
