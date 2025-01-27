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

#[cfg(feature = "file_io")]
use crate::openssl::{EcValidator, EdValidator, RsaValidator};
use crate::Result;

use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct ValidationInfo {
    pub alg: String, // validation algorithm
    pub date: Option<DateTime<Utc>>,
    pub issuer_org: Option<String>,
    pub validated: bool, // claim signature is valid
}

impl Default for ValidationInfo {
    fn default() -> Self {
        ValidationInfo {
            alg: "".to_owned(),
            date: None,
            issuer_org: None,
            validated: false,
        }
    }
}

/// Trait to support validating a signature against the provided data
pub(crate) trait CoseValidator {
    /// validate signature "sig" for given "data using provided public key"
    fn validate(&self, sig: &[u8], data: &[u8], pkey: &[u8]) -> Result<bool>;
}

pub struct DummyValidator;
impl CoseValidator for DummyValidator {
    fn validate(&self, _sig: &[u8], _data: &[u8], _pkey: &[u8]) -> Result<bool> {
        println!("This signature verified by DummyValidator.  Results not valid!");
        Ok(true)
    }
}

// C2PA Supported Signature type
// • ES256 (ECDSA using P-256 and SHA-256)
// • ES384 (ECDSA using P-384 and SHA-384)
// • ES512 (ECDSA using P-521 and SHA-512)
// • PS256 (RSASSA-PSS using SHA-256 and MGF1 with SHA-256)
// • PS384 (RSASSA-PSS using SHA-384 and MGF1 with SHA-384)
// • PS512 (RSASSA-PSS using SHA-512 and MGF1 with SHA-512)
// • RS256	RSASSA-PKCS1-v1_5 using SHA-256
// • RS384	RSASSA-PKCS1-v1_5 using SHA-384
// • RS512	RSASSA-PKCS1-v1_5 using SHA-512
// • ED25519 Edwards Curve ED25519

/// return validator for supported C2PA  algorthms
#[cfg(feature = "file_io")]
pub(crate) fn get_validator(alg: &str) -> Option<Box<dyn CoseValidator>> {
    match alg.to_lowercase().as_str() {
        "es256" => Some(Box::new(EcValidator::new("es256"))),
        "es384" => Some(Box::new(EcValidator::new("es384"))),
        "es512" => Some(Box::new(EcValidator::new("es512"))),
        "ps256" => Some(Box::new(RsaValidator::new("ps256"))),
        "ps384" => Some(Box::new(RsaValidator::new("ps384"))),
        "ps512" => Some(Box::new(RsaValidator::new("ps512"))),
        "rs256" => Some(Box::new(RsaValidator::new("rs256"))),
        "rs384" => Some(Box::new(RsaValidator::new("rs384"))),
        "rs512" => Some(Box::new(RsaValidator::new("rs512"))),
        "ed25519" => Some(Box::new(EdValidator::new("ed25519"))),
        _ => None,
    }
}

#[cfg(not(feature = "file_io"))]
#[allow(dead_code)]
pub(crate) fn get_validator(_alg: &str) -> Option<Box<dyn CoseValidator>> {
    Some(Box::new(DummyValidator))
}
