# Using the Rust library

## Supported platforms

The C2PA Rust library has been tested on the following operating systems:

* Windows (Intel only)
* MacOS (Intel and Apple silicon)
* Ubuntu Linux (64-bit Intel and ARM v8)
* WebAssembly (Wasm)

## Requirements

The C2PA Rust library requires **Rust version 1.76.0** or newer.

To use the library, add this to your `Cargo.toml`:

```toml
[dependencies]
c2pa = "0.36.1"
```

To read or write a manifest file, add the `file_io` dependency to your `Cargo.toml`.

<!-- Check whether thumbnail generation has been removed -->

Add the `add_thumbnails` dependency to generate thumbnails for JPEG and PNG files. For example:

```
c2pa = { version = "0.39.0", features = ["file_io", "add_thumbnails"] }
```

NOTE: If you are building for WASM, omit the `file_io` dependency.

## Features

The Rust library crate provides the following capabilities:

* `file_io` enables manifest generation, signing via OpenSSL, and embedding manifests in [supported file formats](supported-formats.md).
* `add_thumbnails` generates thumbnails automatically for JPEG and PNG files. (no longer included with `file_io`)
* `serialize_thumbnails` includes binary thumbnail data in the [Serde](https://serde.rs/) serialization output.
* `no_interleaved_io` forces fully-synchronous I/O; otherwise, the library uses threaded I/O for some operations to improve performance.
* `fetch_remote_manifests` enables the verification step to retrieve externally referenced manifest stores.  External manifests are only fetched if there is no embedded manifest store and no locally adjacent .c2pa manifest store file of the same name.
* `json_schema` is used by `make schema` to produce a JSON schema document that represents the `ManifestStore` data structures.
* `psxxx_ocsp_stapling_experimental` enables a demonstration feature that attempts to fetch the OCSP data from the OCSP responders listed in the manifest signing certificate.  The response becomes part of the manifest and is used to prove the certificate was not revoked at the time of signing.  This is only implemented for PS256, PS384 and PS512 signatures and is intended as a demonstration.
* `openssl_ffi_mutex` prevents multiple threads from accessing the C OpenSSL library simultaneously. (This library is not re-entrant.) In a multi-threaded process (such as Cargo's test runner), this can lead to unpredictable behavior.

### New API

### Enabling 

<!-- This requirement should go away with actual 1.0 release -->

The current release has a new API that replaces the previous methods of reading and writing C2PA data, which are still supported but will be deprecated.  

To use the new API, enable the `unstable_api` feature; for example:

```
c2pa = {version="0.39.0", features=["unstable_api"]}
```

When version 1.0 of the library is released, the new API will become the default, but you will still be able to use the deprecated API by enabling the `v1_api` feature; for example:

```
c2pa = {version="0.39.0", features=["v1_api"]}
```

### Resource references

A resource reference is a superset of a `HashedUri`, which the C2PA specification refers to  as both `hashed-uri-map` and  `hashed-ext-uri-map`. In some cases either can be used.

A resource reference also adds local references to things like the file system or any abstracted storage. You can use the identifier field to distinguish from the URL field, but they are really the same. However, the specification will only allow for JUMBF and HTTP(S) references, so if the external identifier is not HTTP(S), it must be converted to a JUMBF reference before embedding into a manifest.

When defining a resource for the [ManifestStoreBuilder](https://docs.rs/c2pa/latest/c2pa/struct.Builder.html), existing resources in other manifests may be identified via JUMBF URLs. This allows a new manifest to inherit an existing thumbnail and is also used to reference parent ingredients. The API will generally do this resolution as needed, so you do not need to know about JUMBF URL references when creating a manifest.

The specification often requires adding a `HashedUri` to an assertion. Since the JUMBF URIs for a new manifest are not known when defining the manifest, this creates a "chicken and egg" scenario, resolved with local resource references. When constructing the JUMBF for a manifest, the library converts all local URI references into JUMBF references and corrects the the associated cross references.

URI schemes in a resource reference can have the following forms:
- `self#jumbf` - An internal JUMBF reference
- `file:///` - A local file reference
- `app://contentauth/` - A working store reference
- `http://` - Remote URI
- `https://` - Remote secure URI

Note that the `file:` and `app:` schemes are only used in the context of ManifestStoreBuilder and will never be in JUMBF data. This is proposal, currently there is no implementation for file or app schemes and we do not yet handle http/https schemes this way.

<!-- Is the above still true? "This is proposal, currently there is no implementation" -->

When `file_io` is enabled, the lack of a scheme will be interpreted as a `file:///` reference, otherwise as an `app:` reference.

### Source asset vs parent asset

The source asset isn't always the parent asset: The source asset is the asset that is hashed and signed. It can be the output from an editing application that has not preserved the manifest store from the parent. In that case, the application should have extracted a parent ingredient from the parent asset and added that to the manifest definition. 

- Parent asset: with a manifest store.
- Parent ingredient: generated from that parent asset (hashed and validated)
- Source asset: may be a generated rendition after edits from the parent asset (manifest?)
- Signed output which will include the source asset, and the new manifest store with parent ingredient.

If there is no parent ingredient defined, and the source has a manifest store, the sdk will generate a parent ingredient from the parent.

### Remote URLs and embedding

The default operation of C2PA signing is to embed a C2PA manifest store into an asset. The library also returns the C2PA manifest store so that it can be written to a sidecar or uploaded to a remote service.
- The API supports embedding a remote URL reference into the asset. 
- The remote URL is stored in different ways depending on the asset, but is often stored in XMP data.
- The remote URL must be added to the asset before signing so that it can be hashed along with the asset.
- Not all file formats support embedding remote URLs or embedding manifests stores.
- If you embed a manifest or a remote URL, a new asset will be created with the new data embedded.
- If you don't embed, then the original asset is unmodified and there is no need to write one out.
- The remote URL can be set with `builder.remote_url`.
- If embedding is not needed, set the `builder.no_embed` flag to `true`.


## Example code

The [sdk/examples](https://github.com/contentauth/c2pa-rs/tree/main/sdk/examples) directory contains some minimal example code.  The [client/client.rs](https://github.com/contentauth/c2pa-rs/blob/main/sdk/examples/client/client.rs) is the most instructive and provides and example of reading the contents of a manifest store, recursively displaying nested manifests.
