//! Apple Vision `VNRecognizeTextRequest` → layout-preserving receipt text.
//!
//! Offline / on-device only. Bounding boxes drive Y-sort + tab join in [`layout`].

use objc2::{AnyThread, ClassType};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation,
    VNRequestTextRecognitionLevel,
};

use super::layout::{assemble_layout_text, OcrToken};

/// Recognize text in an image (JPEG/PNG bytes) and return layout-preserving lines.
pub fn recognize_layout_text(image_bytes: &[u8]) -> Result<String, String> {
    if image_bytes.is_empty() {
        return Err("empty image".into());
    }
    if image_bytes.len() > 12 * 1024 * 1024 {
        return Err("image too large".into());
    }

    let data = NSData::with_bytes(image_bytes);
    let options: objc2::rc::Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &options,
    );

    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    // Prefer raw OCR for receipts (amounts stay literal).
    request.setUsesLanguageCorrection(false);

    let requests = NSArray::from_slice(&[request.as_ref()]);
    handler
        .performRequests_error(&requests)
        .map_err(|_| "vision performRequests failed".to_string())?;

    let Some(results) = request.results() else {
        return Ok(String::new());
    };

    let mut tokens = Vec::with_capacity(results.len());
    for obs in results.iter() {
        let obs: &VNRecognizedTextObservation = &*obs;
        let candidates = obs.topCandidates(1);
        let Some(top) = candidates.firstObject() else {
            continue;
        };
        let text = top.string().to_string();
        if text.trim().is_empty() {
            continue;
        }
        // Normalized Vision coords: origin bottom-left.
        let bbox = unsafe { obs.boundingBox() };
        let y_center = bbox.origin.y + bbox.size.height * 0.5;
        tokens.push(OcrToken {
            text,
            y_center,
            x_min: bbox.origin.x,
        });
    }

    Ok(assemble_layout_text(tokens))
}

// Keep ClassType live (Zero Warnings across feature matrices).
const _: fn() -> &'static objc2::runtime::AnyClass = VNRecognizeTextRequest::class;
