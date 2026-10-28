//! macOS: a file opened from Finder (double-click, Open With, a drop on the Dock icon) comes
//! as an "open documents" Apple event, not on the command line, while Drift starts or while it
//! runs, and winit doesn't pass it on. A handler for that event, installed as the app finishes
//! launching (AppKit puts in its own before then, and the event for the file Drift was started
//! with comes right after), hands each file to `drift_app::queue_open`.
//!
//! The workspace forbids `unsafe`, so the few Cocoa methods used are declared below with the
//! signatures Apple documents, which lets them be called as safe Rust.

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{define_class, extern_class, extern_methods, sel};
use objc2_foundation::NSString;

/// An Apple event code from its four characters ('aevt', 'odoc', …).
const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

extern_class!(
    // SAFETY: NSAppleEventManager inherits from NSObject.
    #[unsafe(super(NSObject))]
    #[name = "NSAppleEventManager"]
    struct AppleEventManager;
);

impl AppleEventManager {
    extern_methods!(
        // SAFETY: as Apple documents them.
        #[unsafe(method(sharedAppleEventManager))]
        fn shared() -> Retained<Self>;

        // SAFETY: `selector` must name a method of `handler` taking two event descriptors;
        // the one caller passes this file's handler and its method.
        #[unsafe(method(setEventHandler:andSelector:forEventClass:andEventID:))]
        fn set_event_handler(&self, handler: &NSObject, selector: Sel, class: u32, id: u32);
    );
}

extern_class!(
    // SAFETY: NSAppleEventDescriptor inherits from NSObject.
    #[unsafe(super(NSObject))]
    #[name = "NSAppleEventDescriptor"]
    struct EventDescriptor;
);

impl EventDescriptor {
    extern_methods!(
        // SAFETY: as Apple documents them (AEKeyword and DescType are four-character codes).
        #[unsafe(method(paramDescriptorForKeyword:))]
        fn param(&self, keyword: u32) -> Option<Retained<Self>>;

        #[unsafe(method(numberOfItems))]
        fn count(&self) -> isize;

        #[unsafe(method(descriptorAtIndex:))]
        fn item(&self, index: isize) -> Option<Retained<Self>>;

        #[unsafe(method(coerceToDescriptorType:))]
        fn coerce(&self, kind: u32) -> Option<Retained<Self>>;

        #[unsafe(method(data))]
        fn data(&self) -> Retained<NSObject>;
    );
}

extern_class!(
    // SAFETY: NSURL inherits from NSObject.
    #[unsafe(super(NSObject))]
    #[name = "NSURL"]
    struct Url;
);

impl Url {
    extern_methods!(
        // SAFETY: as Apple documents them; `data` is an NSData (a descriptor's data).
        #[unsafe(method(URLWithDataRepresentation:relativeToURL:))]
        fn from_data(data: &NSObject, base: Option<&Url>) -> Option<Retained<Self>>;

        #[unsafe(method(path))]
        fn path(&self) -> Option<Retained<NSString>>;
    );
}

extern_class!(
    // SAFETY: NSNotificationCenter inherits from NSObject.
    #[unsafe(super(NSObject))]
    #[name = "NSNotificationCenter"]
    struct NotificationCenter;
);

impl NotificationCenter {
    extern_methods!(
        // SAFETY: as Apple documents them; the observer outlives the process (it is leaked).
        #[unsafe(method(defaultCenter))]
        fn default_center() -> Retained<Self>;

        #[unsafe(method(addObserver:selector:name:object:))]
        fn add_observer(
            &self,
            observer: &NSObject,
            selector: Sel,
            name: Option<&NSString>,
            object: Option<&AnyObject>,
        );
    );
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and OpenFiles has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "DriftOpenFiles"]
    struct OpenFiles;

    impl OpenFiles {
        /// NSApplicationWillFinishLaunchingNotification: take the open-documents event.
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &NSObject) {
            AppleEventManager::shared().set_event_handler(
                self,
                sel!(openDocuments:withReplyEvent:),
                code(b"aevt"),
                code(b"odoc"),
            );
        }

        /// The open-documents event: its direct object lists the files.
        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &EventDescriptor, _reply: &EventDescriptor) {
            for path in paths(event) {
                drift_app::queue_open(path);
            }
        }
    }
);

impl OpenFiles {
    extern_methods!(
        // SAFETY: NSObject's `new`.
        #[unsafe(method(new))]
        fn new() -> Retained<Self>;
    );
}

/// The files an open-documents event names (its direct object: a list of file URLs).
fn paths(event: &EventDescriptor) -> Vec<PathBuf> {
    let Some(list) = event.param(code(b"----")) else {
        return Vec::new();
    };
    (1..=list.count())
        .filter_map(|i| {
            let url = list.item(i)?.coerce(code(b"furl"))?;
            let path = Url::from_data(&url.data(), None)?.path()?;
            Some(PathBuf::from(path.to_string()))
        })
        .collect()
}

/// Listen for files opened from Finder. Call before the event loop starts.
pub fn listen_for_opened_files() {
    let observer = OpenFiles::new();
    NotificationCenter::default_center().add_observer(
        &observer,
        sel!(willFinishLaunching:),
        Some(&NSString::from_str(
            "NSApplicationWillFinishLaunchingNotification",
        )),
        None,
    );
    // The notification centre doesn't keep its observers: this one stays for good.
    std::mem::forget(observer);
}
