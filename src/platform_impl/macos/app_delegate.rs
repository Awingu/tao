// Copyright 2019-2021 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0

use crate::{platform::macos::ActivationPolicy, platform_impl::platform::{app_state::AppState, event::EventWrapper}};

use cocoa::base::id;
use objc::{
  declare::ClassDecl,
  runtime::{Class, Object, Sel},
};
use std::{
  cell::{RefCell, RefMut},
  os::raw::c_void,
};

static AUX_DELEGATE_STATE_NAME: &str = "auxState";

pub struct AuxDelegateState {
  /// We store this value in order to be able to defer setting the activation policy until
  /// after the app has finished launching. If the activation policy is set earlier, the
  /// menubar is initially unresponsive on macOS 10.15 for example.
  pub activation_policy: ActivationPolicy,

  pub create_default_menu: bool,
}

pub struct AppDelegateClass(pub *const Class);
unsafe impl Send for AppDelegateClass {}
unsafe impl Sync for AppDelegateClass {}

lazy_static! {
  pub static ref APP_DELEGATE_CLASS: AppDelegateClass = unsafe {
    let superclass = class!(NSResponder);
    let mut decl = ClassDecl::new("TaoAppDelegate", superclass).unwrap();

    decl.add_class_method(sel!(new), new as extern "C" fn(&Class, Sel) -> id);
    decl.add_method(sel!(dealloc), dealloc as extern "C" fn(&Object, Sel));

    decl.add_method(
      sel!(applicationDidFinishLaunching:),
      did_finish_launching as extern "C" fn(&Object, Sel, id),
    );
    decl.add_method(
      sel!(applicationWillFinishLaunching:),
      will_finish_launching as extern "C" fn(&Object, Sel, id),
    );
    decl.add_method(
      sel!(handleUrlEvent:withReplyEvent:),
      handle_url_event_with_reply_event as extern "C" fn(&Object, Sel, id, id),
    );
    decl.add_method(
      sel!(applicationWillTerminate:),
      application_will_terminate as extern "C" fn(&Object, Sel, id),
    );
    decl.add_ivar::<*mut c_void>(AUX_DELEGATE_STATE_NAME);

    AppDelegateClass(decl.register())
  };
}

/// Safety: Assumes that Object is an instance of APP_DELEGATE_CLASS
pub unsafe fn get_aux_state_mut(this: &Object) -> RefMut<'_, AuxDelegateState> {
  let ptr: *mut c_void = *this.get_ivar(AUX_DELEGATE_STATE_NAME);
  // Watch out that this needs to be the correct type
  (*(ptr as *mut RefCell<AuxDelegateState>)).borrow_mut()
}

extern "C" fn new(class: &Class, _: Sel) -> id {
  unsafe {
    let this: id = msg_send![class, alloc];
    let this: id = msg_send![this, init];
    (*this).set_ivar(
      AUX_DELEGATE_STATE_NAME,
      Box::into_raw(Box::new(RefCell::new(AuxDelegateState {
        activation_policy: ActivationPolicy::Regular,
        create_default_menu: true,
      }))) as *mut c_void,
    );
    this
  }
}

extern "C" fn dealloc(this: &Object, _: Sel) {
  unsafe {
    let state_ptr: *mut c_void = *(this.get_ivar(AUX_DELEGATE_STATE_NAME));
    // As soon as the box is constructed it is immediately dropped, releasing the underlying
    // memory
    Box::from_raw(state_ptr as *mut RefCell<AuxDelegateState>);
  }
}

/// Adapted from https://github.com/mrmekon/fruitbasket
/// Apple kInternetEventClass constant
#[allow(non_upper_case_globals)]
const kInternetEventClass: u32 = 0x4755524c;
/// Adapted from https://github.com/mrmekon/fruitbasket
/// Apple kAEGetURL constant
#[allow(non_upper_case_globals)]
const kAEGetURL: u32 = 0x4755524c;
/// Adapted from https://github.com/mrmekon/fruitbasket
/// Apple keyDirectObject constant
#[allow(non_upper_case_globals)]
pub const keyDirectObject: u32 = 0x2d2d2d2d;

extern "C" fn will_finish_launching(this: &Object, _: Sel, _: id) {
  trace!("Triggered `applicationWillFinishLaunching`");
  // Adapted from https://github.com/mrmekon/fruitbasket
  unsafe {
    let cls = Class::get("NSAppleEventManager").unwrap();
    let manager: *mut Object = msg_send![cls, sharedAppleEventManager];
    let _:() = msg_send![
      manager,
      setEventHandler: this
      andSelector: sel!(handleUrlEvent:withReplyEvent:)
      forEventClass: kInternetEventClass
      andEventID: kAEGetURL];
  }
  trace!("Completed `applicationWillFinishLaunching`");
}

/// Adapted from https://github.com/mrmekon/fruitbasket
/// Parse an Apple URL event into a URL string
///
/// Takes an NSAppleEventDescriptor from an Apple URL event, unwraps
/// it, and returns the contained URL as a String.
fn parse_url_event(event: *mut Object) -> String {
  if event as u64 == 0u64 {
      return "".into();
  }
  unsafe {
      let class: u32 = msg_send![event, eventClass];
      let id: u32 = msg_send![event, eventID];
      if class != kInternetEventClass || id != kAEGetURL {
          return "".into();
      }
      let subevent: *mut Object = msg_send![event, paramDescriptorForKeyword: keyDirectObject];
      let nsstring: *mut Object = msg_send![subevent, stringValue];
      nsstring_to_string(nsstring)
  }
}

/// Adapted from https://github.com/mrmekon/fruitbasket
/// Convert an NSString to a Rust `String`
fn nsstring_to_string(nsstring: *mut Object) -> String {
  unsafe {
      let cstr: *const i8 = msg_send![nsstring, UTF8String];
      if cstr != std::ptr::null() {
          std::ffi::CStr::from_ptr(cstr)
              .to_string_lossy()
              .into_owned()
      } else {
          "".into()
      }
  }
}

extern "C" fn handle_url_event_with_reply_event(_: &Object, _: Sel, event: id, _: id) {
  trace!("Triggered `handle_url_event_with_reply_event`");
  let url = parse_url_event(event);
  AppState::queue_event(EventWrapper::StaticEvent(crate::event::Event::UrlEvent(url)));
  trace!("Completed `handle_url_event_with_reply_event`");
}

extern "C" fn did_finish_launching(this: &Object, _: Sel, _: id) {
  trace!("Triggered `applicationDidFinishLaunching`");
  AppState::launched(this);
  trace!("Completed `applicationDidFinishLaunching`");
}

extern "C" fn application_will_terminate(_: &Object, _: Sel, _: id) {
  trace!("Triggered `applicationWillTerminate`");
  AppState::exit();
  trace!("Completed `applicationWillTerminate`");
}
