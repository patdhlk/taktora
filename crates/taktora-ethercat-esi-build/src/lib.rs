//! Build-script glue for the `EtherCAT` ESI device-driver toolchain.
//!
//! A downstream consumer's `build.rs` calls a one-line [`Builder`] to turn its
//! `esi/*.xml` files into a generated Rust module in `OUT_DIR`:
//!
//! ```no_run
//! taktora_ethercat_esi_build::Builder::new()
//!     .glob("esi/*.xml")
//!     .out_file("devices.rs")
//!     .build()
//!     .unwrap();
//! ```
//!
//! [`Builder::build`] reads `OUT_DIR` / `CARGO_MANIFEST_DIR` from the
//! environment and delegates to the testable seam [`Builder::run`], which does
//! the glob → parse → merge → generate → format → write work and returns the
//! matched input paths. `build` then prints the `cargo:rerun-if-changed`
//! directives for each input plus the build script itself (`REQ_0542`).

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use taktora_ethercat_esi::{EsiFile, Vendor};
use taktora_ethercat_esi_codegen::{CodegenBackend, CodegenError, generate};

pub use taktora_ethercat_esi_codegen_ethercrab::EthercrabBackend;

/// Errors raised while turning ESI XML files into a generated module.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// A required environment variable (`OUT_DIR` / `CARGO_MANIFEST_DIR`) was
    /// not set. Only reachable from [`Builder::build`]; the testable
    /// [`Builder::run`] takes both paths as arguments.
    #[error("environment variable `{0}` is not set")]
    MissingEnv(&'static str),

    /// The glob pattern could not be compiled.
    #[error("invalid glob pattern `{pattern}`: {source}")]
    Glob {
        /// The offending glob pattern.
        pattern: String,
        /// The underlying glob compile error.
        #[source]
        source: glob::PatternError,
    },

    /// Reading an input file or writing the generated module failed.
    #[error("I/O error for `{path}`: {source}")]
    Io {
        /// The path involved in the failed operation.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Decoding a matched ESI file's bytes into UTF-8 (honouring its declared
    /// `<?xml … encoding?>`) produced malformed sequences. Treated as a hard
    /// error — a real Beckhoff/vendor ESI file should decode cleanly under its
    /// own declared encoding, so `had_errors` signals a corrupt file or a wrong
    /// declaration rather than something to silently lossy-decode past.
    #[error("failed to decode ESI file `{path}` as `{encoding}`: malformed byte sequence")]
    Decode {
        /// The ESI file that failed to decode.
        path: PathBuf,
        /// The encoding label used to decode (declared, or `UTF-8` by default).
        encoding: &'static str,
    },

    /// Parsing one of the matched ESI files failed.
    #[error("failed to parse ESI file `{path}`: {source}")]
    Parse {
        /// The ESI file that failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: taktora_ethercat_esi::EsiError,
    },

    /// Generating Rust tokens from the parsed device set failed.
    #[error(transparent)]
    Codegen(#[from] CodegenError),

    /// The generated token stream did not parse as a `syn::File`, so it could
    /// not be formatted. This indicates a codegen bug rather than user error.
    #[error("generated tokens did not parse as Rust source: {0}")]
    Format(#[from] syn::Error),
}

/// One-line build-script helper: glob ESI XML files, generate a Rust module,
/// and write it to `OUT_DIR`.
///
/// The backend type defaults to [`EthercrabBackend`]; override it with
/// [`Builder::backend`].
///
/// The glob may match ESI files from multiple vendors: codegen keys every
/// device off its own per-device `Identity`, so file-level vendor is not
/// consulted; the combined `EsiFile.vendor` is kept from the first input only
/// for completeness.
pub struct Builder<B = EthercrabBackend> {
    pattern: String,
    out_file: String,
    backend: B,
}

impl Builder<EthercrabBackend> {
    /// Start a builder with the default [`EthercrabBackend`].
    ///
    /// Defaults: pattern `esi/*.xml`, output file `devices.rs`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pattern: "esi/*.xml".to_owned(),
            out_file: "devices.rs".to_owned(),
            backend: EthercrabBackend,
        }
    }
}

impl Default for Builder<EthercrabBackend> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: CodegenBackend> Builder<B> {
    /// Set the glob pattern, relative to `CARGO_MANIFEST_DIR` (e.g.
    /// `"esi/*.xml"`).
    #[must_use]
    pub fn glob(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = pattern.into();
        self
    }

    /// Set the output file name, written into `OUT_DIR` (e.g. `"devices.rs"`).
    #[must_use]
    pub fn out_file(mut self, name: impl Into<String>) -> Self {
        self.out_file = name.into();
        self
    }

    /// Override the default codegen backend.
    #[must_use]
    pub fn backend<B2: CodegenBackend>(self, backend: B2) -> Builder<B2> {
        Builder {
            pattern: self.pattern,
            out_file: self.out_file,
            backend,
        }
    }

    /// Run the build from a consumer `build.rs`.
    ///
    /// Reads `OUT_DIR` and `CARGO_MANIFEST_DIR` from the environment, runs the
    /// glob → parse → merge → generate → format → write pipeline via
    /// [`Builder::run`], then prints the `cargo:rerun-if-changed` directives
    /// for each matched ESI file and for the build script itself (`REQ_0542`).
    ///
    /// # Errors
    ///
    /// Returns a [`BuildError`] if an environment variable is missing, the glob
    /// pattern is invalid, an I/O operation fails, an ESI file fails to parse,
    /// codegen fails, or the generated tokens do not format.
    pub fn build(&self) -> Result<(), BuildError> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|_| BuildError::MissingEnv("CARGO_MANIFEST_DIR"))?;
        let out_dir = std::env::var("OUT_DIR").map_err(|_| BuildError::MissingEnv("OUT_DIR"))?;

        let inputs = self.run(Path::new(&manifest_dir), Path::new(&out_dir))?;
        for input in &inputs {
            println!("cargo:rerun-if-changed={}", input.display());
        }
        // Re-run when the consumer's build script itself changes (`REQ_0542`).
        println!(
            "cargo:rerun-if-changed={}",
            Path::new(&manifest_dir).join("build.rs").display()
        );
        Ok(())
    }

    /// Testable core: glob `<manifest_dir>/<pattern>`, parse every match (in
    /// sorted order for determinism), merge into one [`EsiFile`], generate and
    /// format the module, and write it to `<out_dir>/<out_file>`.
    ///
    /// Returns the matched input paths (the `rerun-if-changed` targets). Takes
    /// both directories as arguments so tests need not mutate the environment.
    ///
    /// # Errors
    ///
    /// See [`Builder::build`].
    pub fn run(&self, manifest_dir: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, BuildError> {
        let pattern = manifest_dir.join(&self.pattern);
        let pattern = pattern.to_string_lossy();

        let mut inputs: Vec<PathBuf> = glob::glob(&pattern)
            .map_err(|source| BuildError::Glob {
                pattern: pattern.into_owned(),
                source,
            })?
            // A path matched the pattern but failed to stat (permissions, a
            // broken symlink, ...). Surface it as an actionable I/O error
            // rather than silently dropping the input.
            .map(|entry| {
                entry.map_err(|err| BuildError::Io {
                    path: err.path().to_path_buf(),
                    source: err.into_error(),
                })
            })
            .collect::<Result<_, _>>()?;
        // Sort for deterministic merge / output across platforms.
        inputs.sort();

        let combined = Self::merge(&inputs)?;
        let tokens = generate(&combined, &self.backend)?;
        let file: syn::File = syn::parse2(tokens)?;
        let source = prettyplease::unparse(&file);

        let out_path = out_dir.join(&self.out_file);
        std::fs::write(&out_path, source).map_err(|source| BuildError::Io {
            path: out_path,
            source,
        })?;

        Ok(inputs)
    }

    /// Decode an ESI file's raw bytes into UTF-8, honouring the encoding named
    /// in its `<?xml … encoding="LABEL"?>` declaration.
    ///
    /// The declaration is ASCII, so [`sniff_encoding`] scans the leading bytes
    /// for `encoding="…"` and maps the label via
    /// [`encoding_rs::Encoding::for_label`]; an absent declaration or an unknown
    /// label falls back to UTF-8. [`encoding_rs::Encoding::decode`] strips a BOM
    /// if present and signals malformed input via `had_errors`, which is treated
    /// as a hard [`BuildError::Decode`] (see that variant's docs).
    fn decode(path: &Path, bytes: &[u8]) -> Result<String, BuildError> {
        let encoding = sniff_encoding(bytes);
        let (text, _, had_errors) = encoding.decode(bytes);
        if had_errors {
            return Err(BuildError::Decode {
                path: path.to_path_buf(),
                encoding: encoding.name(),
            });
        }
        Ok(text.into_owned())
    }

    /// Read and parse every input, merging all devices into one [`EsiFile`].
    ///
    /// The combined [`EsiFile::vendor`] is taken from the first input purely for
    /// completeness (and to future-proof an `emit_module_root`); codegen
    /// currently ignores file-level vendor entirely, since each device's
    /// `Identity` (`taktora_ethercat_esi_codegen::Identity`) carries its own
    /// vendor/product/revision. Globbing ESI files from multiple vendors is
    /// therefore permitted and harmless. An empty input set yields a default
    /// vendor with no devices (a valid empty module).
    fn merge(inputs: &[PathBuf]) -> Result<EsiFile, BuildError> {
        let mut vendor: Option<Vendor> = None;
        let mut devices = Vec::new();

        for path in inputs {
            // Real Beckhoff ESI files are not UTF-8 (they declare
            // `encoding="ISO-8859-1"` and carry Latin-1 high bytes), so read
            // raw bytes and decode per the file's own declaration before the
            // parser — which is UTF-8-only by contract (`REQ_0500`) — sees it.
            let bytes = std::fs::read(path).map_err(|source| BuildError::Io {
                path: path.clone(),
                source,
            })?;
            let xml = Self::decode(path, &bytes)?;
            let parsed = taktora_ethercat_esi::parse(&xml).map_err(|source| BuildError::Parse {
                path: path.clone(),
                source,
            })?;
            if vendor.is_none() {
                vendor = Some(parsed.vendor);
            }
            devices.extend(parsed.devices);
        }

        Ok(EsiFile {
            vendor: vendor.unwrap_or(Vendor { id: 0, name: None }),
            devices,
            modules: Vec::new(),
        })
    }
}

/// Sniff the encoding from an XML declaration's `encoding="LABEL"` attribute.
///
/// The XML declaration is required to be ASCII regardless of the document
/// encoding, so it is safe to scan the leading bytes directly. Only the first
/// `SNIFF_LEN` bytes are inspected (the declaration, if any, is at the very
/// start). The extracted label is resolved via
/// [`encoding_rs::Encoding::for_label`]; a missing declaration, a malformed one,
/// or an unknown label all fall back to UTF-8.
fn sniff_encoding(bytes: &[u8]) -> &'static encoding_rs::Encoding {
    /// How many leading bytes to scan for the XML declaration.
    const SNIFF_LEN: usize = 200;

    let head = &bytes[..bytes.len().min(SNIFF_LEN)];
    sniff_label(head)
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8)
}

/// Extract the value of an `encoding="…"` (or `encoding='…'`) attribute from the
/// ASCII prefix of an XML declaration, if present.
fn sniff_label(head: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(head).ok().or_else(|| {
        // A high byte appears before the (ASCII) declaration ends — unusual, but
        // still try the ASCII-clean leading run so a declaration is not missed.
        let ascii_end = head.iter().position(|&b| b >= 0x80).unwrap_or(head.len());
        std::str::from_utf8(&head[..ascii_end]).ok()
    })?;

    let after = &text[text.find("encoding")?.saturating_add("encoding".len())..];
    let after = after.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}
