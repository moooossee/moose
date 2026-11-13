use crate::i18n::gettext;

use crate::attachments::{MAX_FILE_BYTES, error};
use crate::error::Result;
use gtk::{gdk, glib, prelude::*};
use std::time::Duration;

pub(super) async fn read_image(clipboard: &gdk::Clipboard) -> Result<Vec<u8>> {
    let formats = gdk::ContentFormats::for_type(gdk::Texture::static_type())
        .union_deserialize_mime_types()
        .mime_types();
    let mime_types = formats.iter().map(|mime| mime.as_str()).collect::<Vec<_>>();
    let (stream, _) = clipboard
        .read_future(&mime_types, glib::Priority::DEFAULT)
        .await
        .map_err(|_| error(gettext("No image could be read from the clipboard")))?;
    let mut bytes = Vec::new();
    loop {
        glib::timeout_future(Duration::ZERO).await;
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await
            .map_err(|_| error(gettext("No image could be read from the clipboard")))?;
        if chunk.is_empty() {
            if bytes.is_empty() {
                return Err(error(gettext("Copy an image or screenshot first")));
            }
            return Ok(bytes);
        }
        if bytes.len() + chunk.len() > MAX_FILE_BYTES {
            return Err(error(gettext("Choose a file smaller than 25 MB")));
        }
        bytes.extend_from_slice(chunk.as_ref());
    }
}
