//! UIKit document import and export in the selected scene. Imported copies live in the app sandbox.
#![allow(unsafe_code)]
use super::IosSystem;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{
    UIAdaptivePresentationControllerDelegate, UIDocumentPickerDelegate,
    UIDocumentPickerViewController, UIPresentationController,
};
use objc2_uniform_type_identifiers::UTType;
use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
};

/// Result of an iOS document import or export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IosDocumentResult {
    /// URL returned by Files. An imported document is a private copy.
    Selected(PathBuf),
    /// The user dismissed the picker.
    Cancelled,
}

#[derive(Default)]
struct Selection {
    value: RefCell<Option<Result<IosDocumentResult, &'static str>>>,
}

define_class!(
    // SAFETY: the delegate is an NSObject subclass with only main-thread Rust ivars.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Selection]
    #[name = "SergeDocumentPickerDelegate"]
    struct PickerDelegate;
    unsafe impl NSObjectProtocol for PickerDelegate {}
    unsafe impl UIAdaptivePresentationControllerDelegate for PickerDelegate {
        #[unsafe(method(presentationControllerDidDismiss:))]
        fn dismissed(&self, _: &UIPresentationController) {
            self.ivars()
                .value
                .borrow_mut()
                .get_or_insert(Ok(IosDocumentResult::Cancelled));
        }
    }
    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let value = urls
                .firstObject()
                .and_then(|url| url.to_file_path())
                .map(IosDocumentResult::Selected)
                .ok_or("picker did not return a file URL");
            self.ivars().value.replace(Some(value));
        }
        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _: &UIDocumentPickerViewController) {
            self.ivars()
                .value
                .replace(Some(Ok(IosDocumentResult::Cancelled)));
        }
    }
);

/// Keeps the weak UIKit delegate alive until the user selects or cancels a document.
pub struct IosDocumentRequest {
    picker: Retained<UIDocumentPickerViewController>,
    delegate: Retained<PickerDelegate>,
}
impl IosDocumentRequest {
    /// Polls on the UI thread without blocking the presentation.
    pub fn try_result(&self) -> io::Result<Option<IosDocumentResult>> {
        // The remote Files UI can be visible before UIKit sets presentingViewController.
        // A delegate callback completes the request on selection or dismissal.
        self.delegate
            .ivars()
            .value
            .borrow()
            .clone()
            .transpose()
            .map_err(io::Error::other)
    }
}
impl Drop for IosDocumentRequest {
    fn drop(&mut self) {
        self.picker.setDelegate(None);
        if self.delegate.ivars().value.borrow().is_none()
            && self.picker.presentingViewController().is_some()
            && !self.picker.isBeingDismissed()
        {
            self.picker
                .dismissViewControllerAnimated_completion(true, None);
        }
    }
}

impl IosSystem {
    /// Presents Files to import a sandbox copy of the chosen UTI, e.g. `public.plain-text`.
    pub fn pick_document(&self, identifier: &str) -> io::Result<IosDocumentRequest> {
        let ty = UTType::typeWithIdentifier(&NSString::from_str(identifier))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown document type"))?;
        let types = NSArray::from_retained_slice(&[ty]);
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
            UIDocumentPickerViewController::alloc(self.main),
            &types,
            true,
        );
        self.present_documents(picker)
    }
    /// Exports a copy of an existing file to a location selected by the user.
    pub fn export_document(&self, path: &Path) -> io::Result<IosDocumentRequest> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a regular file",
            ));
        }
        let url = NSURL::from_file_path(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file URL"))?;
        let urls = NSArray::from_retained_slice(&[url]);
        let picker = UIDocumentPickerViewController::initForExportingURLs_asCopy(
            UIDocumentPickerViewController::alloc(self.main),
            &urls,
            true,
        );
        self.present_documents(picker)
    }
    fn present_documents(
        &self,
        picker: Retained<UIDocumentPickerViewController>,
    ) -> io::Result<IosDocumentRequest> {
        let allocated = PickerDelegate::alloc(self.main).set_ivars(Selection::default());
        // SAFETY: initialize the allocated subclass once via NSObject's designated initializer.
        let delegate: Retained<PickerDelegate> = unsafe { msg_send![super(allocated), init] };
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        self.present(&picker)?;
        if let Some(presentation) = picker.presentationController() {
            // SAFETY: the request owns its main-thread delegate until the picker is dismissed.
            unsafe { presentation.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        }
        Ok(IosDocumentRequest { picker, delegate })
    }
}
