//! macOS: a file opened from Finder (double-click, Open With, a drop on the Dock icon) comes
//! as an "open documents" Apple event, not on the command line, while Drift starts or while it
//! runs, and winit doesn't pass it on. A handler for that event, installed as the app finishes
//! launching (AppKit puts in its own before then, and the event for the file Drift was started
//! with comes right after), hands each file to `drift_app::queue_open`.

use std::ffi::{CStr, c_char};
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{ClassType, class, define_class, msg_send, sel};

/// An Apple event code from its four characters ('aevt', 'odoc', …).
const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and OpenFiles has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "DriftOpenFiles"]
    struct OpenFiles;

    impl OpenFiles {
        /// NSApplicationWillFinishLaunchingNotification: take the open-documents event.
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &AnyObject) {
            // SAFETY: the selector is one of this class's, taking two event descriptors.
            unsafe {
                let manager: Retained<AnyObject> =
                    msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
                let _: () = msg_send![
                    &*manager,
                    setEventHandler: self,
                    andSelector: sel!(openDocuments:withReplyEvent:),
                    forEventClass: code(b"aevt"),
                    andEventID: code(b"odoc")
                ];
            }
        }

        /// The open-documents event: its direct object lists the files.
        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &AnyObject, _reply: &AnyObject) {
            // SAFETY: `event` is an NSAppleEventDescriptor.
            for path in unsafe { paths(event) } {
                drift_app::queue_open(path);
            }
        }
    }
);

/// The files an open-documents event names (its direct object: a list of file URLs).
unsafe fn paths(event: &AnyObject) -> Vec<PathBuf> {
    // SAFETY: the messages are NSAppleEventDescriptor's, NSURL's and NSString's.
    unsafe {
        let list: Option<Retained<AnyObject>> =
            msg_send![event, paramDescriptorForKeyword: code(b"----")];
        let Some(list) = list else {
            return Vec::new();
        };
        let n: isize = msg_send![&*list, numberOfItems];
        (1..=n)
            .filter_map(|i| {
                let item: Option<Retained<AnyObject>> = msg_send![&*list, descriptorAtIndex: i];
                let url: Option<Retained<AnyObject>> =
                    msg_send![&*item?, coerceToDescriptorType: code(b"furl")];
                let data: Option<Retained<AnyObject>> = msg_send![&*url?, data];
                let nsurl: Option<Retained<AnyObject>> = msg_send![
                    class!(NSURL),
                    URLWithDataRepresentation: &*data?,
                    relativeToURL: std::ptr::null::<AnyObject>()
                ];
                let path: *const c_char = msg_send![&*nsurl?, fileSystemRepresentation];
                if path.is_null() {
                    return None;
                }
                CStr::from_ptr(path).to_str().ok().map(PathBuf::from)
            })
            .collect()
    }
}

/// Listen for files opened from Finder. Call before the event loop starts.
pub fn listen_for_opened_files() {
    // SAFETY: plain Foundation messages; the observer lives for the rest of the process (the
    // notification centre doesn't keep it).
    unsafe {
        let observer: Retained<OpenFiles> = msg_send![OpenFiles::class(), new];
        let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
        let name: Retained<AnyObject> = msg_send![
            class!(NSString),
            stringWithUTF8String: c"NSApplicationWillFinishLaunchingNotification".as_ptr()
        ];
        let _: () = msg_send![
            &*center,
            addObserver: &*observer,
            selector: sel!(willFinishLaunching:),
            name: &*name,
            object: std::ptr::null::<AnyObject>()
        ];
        std::mem::forget(observer);
    }
}
