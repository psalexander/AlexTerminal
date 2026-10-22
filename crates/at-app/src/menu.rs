//! macOS application menu: a quit item that goes through the app instead of `terminate:`.
//!
//! winit builds the default application menu whose "Quit" item calls `terminate:`
//! and kills the process without saving the session or asking about busy panes.
//! Here it is replaced by an item that runs the app's own quit action.
//! Picked items reach the app through [`stream`] as [`Message::MenuAction`].

use crate::app::Message;
use at_core::keymap::Action;
use iced::futures::Stream;
use iced::futures::channel::mpsc::{UnboundedSender, unbounded};
use parking_lot::Mutex;

static TX: Mutex<Option<UnboundedSender<Action>>> = Mutex::new(None);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pick(action: Action) {
    if let Some(tx) = TX.lock().as_ref() {
        let _ = tx.unbounded_send(action);
    }
}

pub fn stream() -> impl Stream<Item = Message> {
    use iced::futures::StreamExt;
    let (tx, rx) = unbounded();
    *TX.lock() = Some(tx);
    rx.map(Message::MenuAction)
}

/// Puts the quit item into the application menu. Returns false while the menu
/// does not exist yet (call again later); always true off macOS.
#[cfg(not(target_os = "macos"))]
pub fn install(_items: &[(Action, String)]) -> bool {
    true
}

#[cfg(target_os = "macos")]
pub use mac::install;

#[cfg(target_os = "macos")]
mod mac {
    use super::pick;
    use at_core::keymap::Action;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSString};

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "AlexTerminalMenuTarget"]
        struct MenuTarget;

        impl MenuTarget {
            #[unsafe(method(quit:))]
            fn quit(&self, _sender: Option<&AnyObject>) {
                pick(Action::Quit);
            }
        }
    );

    pub fn install(items: &[(Action, String)]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        let Some(app_menu) = app
            .mainMenu()
            .and_then(|m| m.itemAtIndex(0))
            .and_then(|i| i.submenu())
        else {
            return false;
        };
        // Menu items hold their target weakly: keep it for the life of the app.
        let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };
        let target: &'static MenuTarget = Box::leak(Box::new(target));

        // Drop winit's `terminate:` item; ours takes its place at the end.
        let n = app_menu.numberOfItems();
        for i in (0..n).rev() {
            if let Some(item) = app_menu.itemAtIndex(i)
                && item.action() == Some(sel!(terminate:))
            {
                app_menu.removeItemAtIndex(i);
            }
        }
        let cmd = NSEventModifierFlags::Command;
        for (action, title) in items {
            let (sel, mods): (Sel, NSEventModifierFlags) = match action {
                Action::Quit => (sel!(quit:), cmd),
                _ => continue,
            };
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(title),
                    Some(sel),
                    &NSString::from_str("q"),
                )
            };
            item.setKeyEquivalentModifierMask(mods);
            unsafe { item.setTarget(Some(target)) };
            app_menu.addItem(&item);
        }
        true
    }
}
