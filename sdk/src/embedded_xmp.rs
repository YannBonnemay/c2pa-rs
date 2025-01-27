// Copyright 2022 Adobe. All rights reserved.
// This file is licensed to you under the Apache License,
// Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// or the MIT license (http://opensource.org/licenses/MIT),
// at your option.

// Unless required by applicable law or agreed to in writing,
// this software is distributed on an "AS IS" BASIS, WITHOUT
// WARRANTIES OR REPRESENTATIONS OF ANY KIND, either express or
// implied. See the LICENSE-MIT and LICENSE-APACHE files for the
// specific language governing permissions and limitations under
// each license.

use std::path::Path;

use xmp_toolkit::{OpenFileOptions, XmpFile, XmpFileError, XmpMeta};

/// Add the URI for the active manifest to the XMP packet for a file.
///
/// This will replace any existing `dc:provenance` term
/// in the file's metadata, or create a new one if necessary.
///
/// This does not check the claim at all; it is presumed
/// that the string that is passed is a valid signed claim.
pub(crate) fn add_manifest_uri_to_file<P: AsRef<Path>>(
    path: P,
    manifest_uri: &str,
) -> Result<(), XmpFileError> {
    XmpMeta::register_namespace("http://purl.org/dc/terms/", "dcterms");

    let mut f = XmpFile::new();

    f.open_file(path, OpenFileOptions::OPEN_FOR_UPDATE)?;

    let mut m = f.xmp().unwrap_or_else(XmpMeta::new);
    m.set_property("http://purl.org/dc/terms/", "provenance", manifest_uri);
    f.put_xmp(&m);
    f.close();

    Ok(())
}
