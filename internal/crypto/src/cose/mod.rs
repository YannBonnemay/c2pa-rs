// Copyright 2024 Adobe. All rights reserved.
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

//! This module provides functions for working with [COSE] signatures.
//!
//! [COSE]: https://datatracker.ietf.org/doc/rfc9052/

mod certificate_acceptance_policy;
pub use certificate_acceptance_policy::{CertificateAcceptancePolicy, InvalidCertificateError};

mod error;
pub use error::CoseError;

mod sigtst;
pub use sigtst::{
    cose_countersign_data, parse_and_validate_sigtst, parse_and_validate_sigtst_async, TstToken,
};
